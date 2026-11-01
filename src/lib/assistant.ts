// The assistant in Alloy: the conversation, the loop that sends it to AlloyMC's relay and
// runs the tools Claude asks for, and the tools themselves.
//
// Tools run here, through the same commands Alloy's own buttons use. Tools that read run
// straight away; tools that change something wait for the player to allow them, and the
// first change to an instance in a conversation makes an automatic save point.

import { create } from "zustand";
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog";
import { api, isTauri, newJobId } from "./api";
import { listen } from "./bus";
import { loaderName } from "./format";
import { useStore } from "./store";
import type { ContentItem, CrashFix, Diagnosis, Instance, Loader, Problem } from "./types";

// ---------- messages ----------

export type Block =
  | { type: "text"; text: string }
  | { type: "tool_use"; id: string; name: string; input: Record<string, any> }
  | {
      type: "tool_result";
      tool_use_id: string;
      content: string;
      is_error?: boolean;
    }
  | { type: "thinking"; thinking: string; signature?: string };

export interface Msg {
  role: "user" | "assistant";
  content: Block[];
}

export type Mode = "fast" | "deep";

export interface AssistantStatus {
  signedIn: boolean;
  available: boolean;
  reason?: string | null;
  needs?: "email" | "minecraft" | null;
  dayUsed: number;
  monthUsed: number;
  dayResets?: string | null;
  unlimited: boolean;
}

export interface ChatSummary {
  id: string;
  title: string;
  instanceId?: string | null;
  updated: number;
}

export interface Chat extends ChatSummary {
  messages: Msg[];
  labels: Record<string, string>;
  mode?: Mode | null;
  undo?: Record<string, UndoEntry>;
  plan?: ApprovedPlan | null;
}

export type Delta = { type: "text"; index: number; text: string } | { type: "tool"; index: number; id: string; name: string };

/** Context sent ahead of the player's words; never shown as theirs. */
export const CONTEXT_TAG = "<alloy-context>";

export interface Approval {
  toolUseId: string;
  name: string;
  title: string;
  detail?: string;
  diff?: DiffLine[];
  /** Deleting always asks, even after "allow the rest". */
  always: boolean;
  /** A batch: what the player can tick or untick. */
  items?: BatchItem[];
  /** A whole pack plan to review. */
  proposal?: PackProposal;
}

export interface PlanItem {
  key: string;
  platform: "modrinth" | "curseforge";
  projectId: string;
  title: string;
  why?: string;
}

export interface PackProposal {
  name: string;
  gameVersion: string;
  loader: string;
  summary: string;
  groups: { name: string; items: PlanItem[] }[];
}

/** A plan the player approved: installs from it don't ask again. */
export interface ApprovedPlan {
  name: string;
  keys: string[];
}

/** What to restore to undo one reply's changes. */
export interface UndoEntry {
  label: string;
  points: { instanceId: string; pointId: string }[];
  undone?: boolean;
}

export interface BatchItem {
  key: string;
  title: string;
  detail?: string;
  /** Can't be installed (no fitting version, already there); shown, not ticked. */
  unavailable?: string;
}

type Answer = {
  kind: "allow" | "decline" | "rest" | "changes";
  selected?: string[];
  note?: string;
};

interface State {
  open: boolean;
  setOpen: (open: boolean) => void;
  /** The instance the conversation is about, if any. */
  instanceId: string | null;
  setInstance: (id: string | null) => void;
  mode: Mode;
  setMode: (m: Mode) => void;

  chatId: string;
  title: string;
  messages: Msg[];
  labels: Record<string, string>;
  /** The reply being written: text by block index and tools by id. */
  live: { blocks: Record<number, { text?: string; tool?: string }> } | null;
  running: string | null;
  busy: boolean;
  approval: Approval | null;
  allowRest: boolean;
  error: string | null;
  /** Save points per reply (keyed by the index of the message that asked), for undo. */
  undo: Record<string, UndoEntry>;
  plan: ApprovedPlan | null;
  undoTurn: (key: string) => Promise<void>;
  /** Plans: approve with what's kept, or ask for changes. */
  answerPlan: (a: { approve: boolean; selected: string[]; note: string }) => void;

  send: (text: string) => Promise<void>;
  stop: () => void;
  answer: (allow: boolean, rest?: boolean, selected?: string[]) => void;
  reset: () => void;
  load: (id: string) => Promise<void>;
  /** Opens the panel and asks straight away, about an instance. */
  ask: (text: string, instanceId?: string | null) => void;
}

const MAX_ROUNDS = 30;
const RESULT_LIMIT = 30_000;

let requestId: string | null = null;
let stopped = false;
let resolveApproval: ((v: Answer) => void) | null = null;
/** The reply in progress: which message asked, and the save points made for it so far. */
let turn: { key: string; label: string; points: Map<string, string> } | null = null;
/** Something to tell Claude with the next message (an undo since). */
let pendingNote: string | null = null;

const fresh = () => ({
  chatId: crypto.randomUUID(),
  title: "",
  messages: [] as Msg[],
  labels: {} as Record<string, string>,
  live: null,
  running: null,
  approval: null,
  allowRest: false,
  error: null,
  undo: {} as Record<string, UndoEntry>,
  plan: null as ApprovedPlan | null,
});

export const useAssistant = create<State>((set, get) => ({
  open: false,
  setOpen: (open) => set({ open }),
  instanceId: null,
  setInstance: (instanceId) => set({ instanceId }),
  mode: "fast",
  setMode: (mode) => set({ mode }),
  busy: false,
  ...fresh(),

  reset: () => {
    if (get().busy) get().stop();
    pendingNote = null;
    set(fresh());
  },

  load: async (id) => {
    if (get().busy) return;
    const c = await api.assistantChat(id);
    pendingNote = null;
    set({
      ...fresh(),
      chatId: c.id,
      title: c.title,
      messages: c.messages,
      labels: c.labels ?? {},
      instanceId: c.instanceId ?? null,
      mode: c.mode ?? get().mode,
      undo: (c.undo ?? {}) as Record<string, UndoEntry>,
      plan: (c.plan ?? null) as ApprovedPlan | null,
    });
  },

  ask: (text, instanceId) => {
    const s = get();
    if (s.busy) {
      set({ open: true });
      return;
    }
    if (s.messages.length > 0) s.reset();
    set({ open: true, instanceId: instanceId ?? s.instanceId });
    void get().send(text);
  },

  stop: () => {
    stopped = true;
    if (requestId) void api.assistantCancel(requestId).catch(() => {});
    resolveApproval?.({ kind: "decline" });
  },

  answer: (allow, rest, selected) => {
    resolveApproval?.({
      kind: allow ? (rest ? "rest" : "allow") : "decline",
      selected,
    });
  },

  answerPlan: ({ approve, selected, note }) => {
    resolveApproval?.({ kind: approve ? "allow" : "changes", selected, note });
  },

  undoTurn: async (key) => {
    const entry = get().undo[key];
    if (!entry || entry.undone) return;
    for (const p of entry.points) await api.savepointRestore(p.instanceId, p.pointId);
    set((s) => ({ undo: { ...s.undo, [key]: { ...entry, undone: true } } }));
    pendingNote = `The player undid all changes from their request "${entry.label}": those instances are back as they were before it.`;
    useStore.getState().toast({ kind: "success", text: "Changes undone" });
    void save(get());
  },

  send: async (text) => {
    const trimmed = text.trim();
    if (!trimmed || get().busy) return;
    stopped = false;
    const ctx = await contextNote(get().instanceId, get().messages.length === 0);
    const blocks: Block[] = [...(ctx ? [{ type: "text" as const, text: ctx }] : []), { type: "text", text: trimmed }];
    turn = {
      key: String(get().messages.length),
      label: trimmed.length > 60 ? `${trimmed.slice(0, 57)}…` : trimmed,
      points: new Map(),
    };
    set((s) => ({
      busy: true,
      error: null,
      title: s.title || trimmed.slice(0, 70),
      messages: pushUser(s.messages, blocks),
    }));
    try {
      for (let round = 0; round < MAX_ROUNDS && !stopped; round++) {
        requestId = newJobId();
        set({ live: { blocks: {} } });
        const reply = await api.assistantSend(requestId, get().mode, get().messages, toolNames());
        requestId = null;
        set((s) => ({
          live: null,
          messages: [...s.messages, { role: "assistant", content: reply.content }],
        }));
        const uses = reply.content.filter((b): b is Extract<Block, { type: "tool_use" }> => b.type === "tool_use");
        if (reply.stopReason !== "tool_use" || uses.length === 0) break;
        const results: Block[] = [];
        for (const u of uses) {
          if (stopped) {
            results.push({
              type: "tool_result",
              tool_use_id: u.id,
              content: "Stopped by the player.",
              is_error: true,
            });
            continue;
          }
          set({ running: u.id });
          results.push(await runTool(u, set, get));
          set({ running: null });
        }
        set((s) => ({ messages: pushUser(s.messages, results) }));
      }
    } catch (e) {
      requestId = null;
      if (!stopped) set({ error: String(e) });
    } finally {
      set({ busy: false, live: null, running: null, approval: null });
      resolveApproval = null;
      void save(get());
    }
  },
}));

/** Appends to the last user message when the conversation already ends with one (after a stop). */
function pushUser(messages: Msg[], blocks: Block[]): Msg[] {
  const last = messages[messages.length - 1];
  if (last?.role === "user") return [...messages.slice(0, -1), { role: "user", content: [...last.content, ...blocks] }];
  return [...messages, { role: "user", content: blocks }];
}

async function save(s: State) {
  if (s.messages.length === 0) return;
  try {
    await api.assistantChatSave({
      id: s.chatId,
      title: s.title || "Conversation",
      instanceId: s.instanceId,
      updated: Math.floor(Date.now() / 1000),
      messages: s.messages,
      labels: s.labels,
      mode: s.mode,
      undo: s.undo,
      plan: s.plan,
    });
  } catch {
    /* saving is a convenience */
  }
}

async function contextNote(instanceId: string | null, first: boolean): Promise<string | null> {
  const page = useStore.getState().page;
  const parts: string[] = [];
  if (pendingNote) {
    parts.push(pendingNote);
    pendingNote = null;
  }
  if (first) {
    const notes = await api.assistantMemory().catch(() => [] as string[]);
    if (notes.length) parts.push(`Notes you saved about the player (forget takes the number): ${notes.map((n, i) => `${i}. ${n}`).join(" ")}`);
  }
  if (instanceId) {
    const inst = await api.instance(instanceId).catch(() => null);
    if (inst)
      parts.push(`The player has the instance "${inst.name}" open (instance_id ${inst.id}, Minecraft ${inst.gameVersion}, ${loaderName[inst.loader]}).`);
  } else if (page.name !== "instance") {
    parts.push(`The player is on Alloy's ${page.name} page.`);
  }
  return parts.length ? `${CONTEXT_TAG}${parts.join(" ")}</alloy-context>` : null;
}

/** Side conversations (the crash doctor's explanation) stream here by request id. */
const sideStreams = new Map<string, (d: Delta) => void>();

// Stream deltas into `live`.
listen("assistant-delta", (e: { requestId: string; delta: Delta }) => {
  sideStreams.get(e.requestId)?.(e.delta);
  if (e.requestId !== requestId) return;
  const d = e.delta;
  useAssistant.setState((s) => {
    if (!s.live) return s;
    const cur = s.live.blocks[d.index] ?? {};
    const next = d.type === "text" ? { ...cur, text: (cur.text ?? "") + d.text } : { ...cur, tool: d.name };
    return { live: { blocks: { ...s.live.blocks, [d.index]: next } } };
  });
});

// ---------- tools ----------

type ToolUse = Extract<Block, { type: "tool_use" }>;
type Setter = (p: Partial<State> | ((s: State) => Partial<State>)) => void;

interface Plan {
  title: string;
  detail?: string;
  diff?: DiffLine[];
  /** `selected`: the batch items the player kept; `progress` relabels the row while it runs. */
  run: (selected?: string[], progress?: (text: string) => void, note?: string) => Promise<string>;
  items?: BatchItem[];
  proposal?: PackProposal;
  /** Already approved (part of an approved plan): runs without asking. */
  preApproved?: boolean;
  /** The instance the change touches, for the save point. */
  instanceId?: string;
  noCheckpoint?: boolean;
}

/** A change that turned out to need nothing: answered without asking. */
class Nothing {
  constructor(public answer: string) {}
}

async function runTool(u: ToolUse, set: Setter, get: () => State): Promise<Block> {
  const result = (content: string, is_error = false): Block => ({
    type: "tool_result",
    tool_use_id: u.id,
    content: cut(content),
    ...(is_error ? { is_error } : {}),
  });
  const label = (text: string) => set((s) => ({ labels: { ...s.labels, [u.id]: text } }));
  try {
    const read = readers[u.name];
    if (read) {
      label(await read.label(u.input));
      return result(await read.run(u.input));
    }
    const change = changers[u.name];
    if (!change) return result(`Alloy has no tool called ${u.name}.`, true);
    let plan: Plan;
    try {
      plan = await change.plan(u.input);
    } catch (e) {
      if (e instanceof Nothing) {
        label(e.answer);
        return result(e.answer);
      }
      throw e;
    }
    label(plan.title);
    const always = u.name === "remove_content" || !!plan.proposal;
    let selected = plan.items?.filter((i) => !i.unavailable).map((i) => i.key);
    let note: string | undefined;
    if ((!get().allowRest && !plan.preApproved) || always) {
      set({
        approval: {
          toolUseId: u.id,
          name: u.name,
          title: plan.title,
          detail: plan.detail,
          diff: plan.diff,
          always,
          items: plan.items,
          proposal: plan.proposal,
        },
      });
      const answer = await new Promise<Answer>((r) => (resolveApproval = r));
      resolveApproval = null;
      set({ approval: null });
      if (answer.kind === "changes") {
        set((s) => ({
          labels: { ...s.labels, [u.id]: `Asked for changes: ${plan.title}` },
        }));
        return result(
          `The player wants changes before approving. Their note: ${answer.note?.trim() || "(none)"}${answer.selected ? `\nThey had ${answer.selected.length} items ticked.` : ""}`,
        );
      }
      if (answer.kind === "decline" || (plan.items && answer.selected?.length === 0)) {
        set((s) => ({
          labels: { ...s.labels, [u.id]: `Declined: ${plan.title}` },
        }));
        return result(stopped ? "Stopped by the player." : "The player declined this.", true);
      }
      if (answer.kind === "rest") set({ allowRest: true });
      if (answer.selected) selected = answer.selected;
      note = answer.note;
    }
    if (plan.instanceId && !plan.noCheckpoint && turn && !turn.points.has(plan.instanceId)) {
      const id = await api.assistantCheckpoint(plan.instanceId, `Before: ${turn.label}`).catch(() => null);
      if (id) {
        turn.points.set(plan.instanceId, id);
        const t = turn;
        set((s) => ({
          undo: {
            ...s.undo,
            [t.key]: {
              label: t.label,
              points: [...t.points].map(([instanceId, pointId]) => ({
                instanceId,
                pointId,
              })),
            },
          },
        }));
      }
    }
    return result(await plan.run(selected, label, note));
  } catch (e) {
    return result(String(e), true);
  }
}

function cut(s: string, max = RESULT_LIMIT) {
  return s.length <= max ? s : `${s.slice(0, max)}\n... (cut, ${s.length - max} more characters)`;
}

const str = (v: unknown) => (typeof v === "string" ? v : "");
const planKey = (platform: unknown, id: unknown) => `${platform === "curseforge" ? "curseforge" : "modrinth"}:${String(id ?? "")}`;

async function instanceNamed(id: string): Promise<Instance> {
  try {
    const inst = await api.instance(id);
    if (inst) return inst;
  } catch {
    /* not an id; maybe a name */
  }
  const all = await api.instances();
  const hit = all.find((i) => i.name.toLowerCase() === id.toLowerCase());
  if (hit) return hit;
  throw `There's no instance with id "${id}". Instances: ${all.map((i) => `${i.name} (${i.id})`).join(", ") || "none"}.`;
}

const nameOf = async (id: string) => (await instanceNamed(id).catch(() => null))?.name ?? id;

const gb = (mb: number) => `${(mb / 1024).toFixed(mb % 1024 ? 1 : 0)} GB`;

function itemLine(i: ContentItem) {
  const version = i.meta?.versionName ?? i.version;
  const bits = [i.kind !== "mod" ? i.kind : null, i.enabled ? null : "off"].filter(Boolean).join(", ");
  return `- ${i.title}${version ? ` ${version}` : ""}${bits ? ` [${bits}]` : ""} · ${i.meta?.source ?? "local"} · ${i.path}${i.modId ? ` · id ${i.modId}` : ""}`;
}

function problemLine(p: Problem) {
  return `- ${p.severity}: ${p.modName}: ${p.message} (${p.path})`;
}

function crashFixLine(f: CrashFix) {
  switch (f.kind) {
    case "raiseMemory":
      return `give it ${gb(f.mb)} of memory (set_memory)`;
    case "useJava":
      return `use Java ${f.major} (the player can apply this in the crash doctor)`;
    case "disableMod":
      return `turn off ${f.path} (set_enabled)`;
  }
}

export function diagnosisText(d: Diagnosis) {
  const lines = [
    `${d.title}. ${d.detail}`,
    `Kind: ${d.kind}. ${d.afterStart ? "It crashed while playing." : "It crashed while starting."}${d.exitCode != null ? ` Exit code ${d.exitCode}.` : ""}`,
  ];
  if (d.suspects.length) {
    lines.push("Suspects:");
    for (const s of d.suspects) lines.push(`- ${s.name} (${s.confidence}): ${s.reason} · ${s.path}`);
  }
  if (d.fixes.length) lines.push(`Fixes the doctor offers: ${d.fixes.map(crashFixLine).join("; ")}.`);
  if (d.evidence.length) lines.push("Key lines:", ...d.evidence.slice(0, 40));
  if (d.canBisect) lines.push("The crash doctor can also find the mod by starting with halves of the mods off (the player starts that in the crash doctor).");
  return lines.join("\n");
}

const readers: Record<string, { label: (i: any) => Promise<string>; run: (i: any) => Promise<string> }> = {
  list_instances: {
    label: async () => "Looked at your instances",
    run: async () => {
      const all = await api.instances();
      if (all.length === 0) return "The player has no instances yet.";
      return all
        .map(
          (i) =>
            `- ${i.name} · id ${i.id} · Minecraft ${i.gameVersion} · ${loaderName[i.loader]}${i.loaderVersion ? ` ${i.loaderVersion}` : ""}${i.linkedPack?.name ? ` · from the pack ${i.linkedPack.name}` : ""}${i.lastPlayed ? ` · last played ${new Date(i.lastPlayed * 1000).toISOString().slice(0, 10)}` : " · never played"}`,
        )
        .join("\n");
    },
  },
  get_instance: {
    label: async (i) => `Checked ${await nameOf(str(i.instance_id))}`,
    run: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      const [ov, fixes, settings] = await Promise.all([api.overview(inst.id), api.fixes(inst.id).catch(() => []), api.getSettings()]);
      const lines = [
        `${inst.name} · id ${inst.id} · Minecraft ${inst.gameVersion} · ${loaderName[inst.loader]}${inst.loaderVersion ? ` ${inst.loaderVersion}` : ""}`,
        `Memory: ${inst.memoryMb ? gb(inst.memoryMb) : `${gb(settings.defaultMemoryMb)} (Alloy's default)`}${inst.javaPath ? ` · Java: ${inst.javaPath}` : ""} · install: ${inst.installState}`,
      ];
      if (inst.linkedPack?.name)
        lines.push(
          `From the modpack ${inst.linkedPack.name}${inst.linkedPack.versionName ? ` ${inst.linkedPack.versionName}` : ""} (${inst.linkedPack.source})`,
        );
      const count = (k: string) => ov.items.filter((x) => x.kind === k).length;
      lines.push(`Content: ${count("mod")} mods, ${count("resourcepack")} resource packs, ${count("shader")} shaders, ${count("datapack")} data packs`);
      lines.push(...ov.items.map(itemLine));
      const errors = ov.report.problems.filter((p) => p.severity === "error");
      const warnings = ov.report.problems.filter((p) => p.severity === "warning");
      lines.push(`Pre-launch check: ${errors.length} errors, ${warnings.length} warnings`);
      lines.push(...errors.map(problemLine), ...warnings.map(problemLine));
      if (fixes.length) {
        lines.push("Fixes Alloy offers (apply_fix with the index):");
        fixes.forEach((f, n) => lines.push(`- ${n}: ${f.label} (${f.message})`));
      }
      if (ov.blocked.length)
        lines.push(
          `Waiting for a manual CurseForge download (the player gets these in Alloy's CurseForge window): ${ov.blocked.map((b) => b.title).join(", ")}`,
        );
      if (ov.listedMissing) lines.push("Some files the modpack lists haven't been downloaded yet.");
      return lines.join("\n");
    },
  },
  search_projects: {
    label: async (i) => `Searched ${i.platform === "curseforge" ? "CurseForge" : "Modrinth"} for “${str(i.query)}”`,
    run: async (i) => {
      const page = await api.search(i.platform === "curseforge" ? "curseforge" : "modrinth", {
        query: str(i.query),
        kind: i.kind ?? "mod",
        gameVersion: i.game_version || undefined,
        loader: i.loader || undefined,
        limit: Math.min(20, i.limit ?? 10),
      });
      if (page.hits.length === 0) return "Nothing found. Try other words, or without the version or loader filter.";
      return [
        `${page.total} results; the first ${page.hits.length}:`,
        ...page.hits.map((h) => `- ${h.title} · project_id ${h.id} · by ${h.author} · ${h.downloads.toLocaleString("en")} downloads · ${h.description}`),
      ].join("\n");
    },
  },
  get_project: {
    label: async () => "Read a project page",
    run: async (i) => (i.platform === "curseforge" ? curseforgeProject(i) : modrinthProject(i)),
  },
  read_log: {
    label: async (i) => `Read ${await nameOf(str(i.instance_id))}'s log${i.contains ? ` for “${i.contains}”` : ""}`,
    run: async (i) => api.assistantReadLog(str(i.instance_id), i.lines, i.contains),
  },
  read_crash: {
    label: async (i) => `Read ${await nameOf(str(i.instance_id))}'s last crash`,
    run: async (i) => {
      const d = await api.crashLast(str(i.instance_id));
      return d ? diagnosisText(d) : "No crash recorded since the instance last started fine.";
    },
  },
  show_projects: {
    label: async (i) => `Showed you ${Array.isArray(i.items) ? i.items.length : 0} project${i.items?.length === 1 ? "" : "s"}`,
    run: async (i) => `Shown to the player as cards with an Add button: ${(Array.isArray(i.items) ? i.items : []).map((x: any) => x.title).join(", ")}.`,
  },
  remember: {
    label: async (i) => `Remembered: ${str(i.note)}`,
    run: async (i) => {
      const notes = await api.assistantMemory();
      const note = str(i.note).trim().slice(0, 200);
      if (!note) throw "Nothing to remember.";
      await api.assistantMemorySet([...notes, note]);
      return `Saved as note ${notes.length}.`;
    },
  },
  forget: {
    label: async () => "Forgot a note",
    run: async (i) => {
      const notes = await api.assistantMemory();
      const n = Number(i.index);
      if (!(n >= 0 && n < notes.length)) throw `There's no note ${i.index}; there are ${notes.length}.`;
      await api.assistantMemorySet(notes.filter((_, k) => k !== n));
      return `Forgot: ${notes[n]}`;
    },
  },
  read_file: {
    label: async (i) => `Read ${str(i.path)}`,
    run: async (i) => api.assistantReadFile(str(i.instance_id), str(i.path)),
  },
};

function plain(md: string, max: number) {
  const text = md
    .replace(/<[^>]+>/g, " ")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/[ \t]+/g, " ")
    .replace(/\n\s*\n+/g, "\n")
    .trim();
  return text.length > max ? `${text.slice(0, max)}…` : text;
}

async function modrinthProject(i: any) {
  const p = await api.modrinthProject(str(i.project_id));
  let versions = await api.modrinthVersions(p.id);
  if (i.game_version) versions = versions.filter((v) => v.game_versions.includes(i.game_version));
  if (i.loader) versions = versions.filter((v) => v.loaders.includes(i.loader));
  const lines = [
    `${p.title} · project_id ${p.id} · ${p.project_type} · ${p.downloads.toLocaleString("en")} downloads`,
    `Runs on: ${p.loaders.join(", ")} · client ${p.client_side ?? "?"}, server ${p.server_side ?? "?"}`,
    `Minecraft versions: ${p.game_versions.slice(-12).join(", ")}${p.game_versions.length > 12 ? ` (and ${p.game_versions.length - 12} older)` : ""}`,
    p.description,
    plain(p.body, 2500),
    `Versions${i.game_version || i.loader ? " matching the filter" : ""} (newest first):`,
  ];
  if (versions.length === 0) lines.push("None.");
  for (const v of versions.slice(0, 6)) {
    const req = v.dependencies.filter((d) => d.dependency_type === "required" && d.project_id).map((d) => d.project_id);
    const breaks = v.dependencies.filter((d) => d.dependency_type === "incompatible" && d.project_id).map((d) => d.project_id);
    lines.push(
      `- ${v.version_number} · version_id ${v.id} · ${v.version_type} · Minecraft ${v.game_versions.slice(-6).join(", ")} · ${v.loaders.join(", ")}${req.length ? ` · requires ${req.join(", ")}` : ""}${breaks.length ? ` · incompatible with ${breaks.join(", ")}` : ""}`,
    );
  }
  return lines.join("\n");
}

const cfLoaders: Record<string, string> = {
  forge: "Forge",
  fabric: "Fabric",
  quilt: "Quilt",
  neoforge: "NeoForge",
};

async function curseforgeProject(i: any) {
  const id = Number(i.project_id);
  if (!Number.isFinite(id)) throw "CurseForge project ids are numbers.";
  const [m, desc, files] = await Promise.all([api.curseforgeProject(id), api.curseforgeDescription(id).catch(() => ""), api.curseforgeFiles(id)]);
  let fs = files;
  if (i.game_version) fs = fs.filter((f) => f.gameVersions.includes(i.game_version));
  if (i.loader && cfLoaders[i.loader]) fs = fs.filter((f) => f.gameVersions.includes(cfLoaders[i.loader]));
  const type = ["release", "beta", "alpha"];
  const lines = [
    `${m.name} · project_id ${m.id} · by ${m.authors.map((a) => a.name).join(", ")} · ${m.downloadCount.toLocaleString("en")} downloads`,
    m.summary,
    plain(desc, 2500),
    `Files${i.game_version || i.loader ? " matching the filter" : ""} (newest first):`,
  ];
  if (fs.length === 0) lines.push("None.");
  for (const f of fs.slice(0, 6)) {
    const req = f.dependencies.filter((d) => d.relationType === 3).map((d) => d.modId);
    lines.push(
      `- ${f.displayName} · version_id ${f.id} · ${type[f.releaseType - 1] ?? "release"} · ${f.gameVersions.join(", ")}${req.length ? ` · requires projects ${req.join(", ")}` : ""}`,
    );
  }
  return lines.join("\n");
}

function outcomeText(o: { installed: string[]; unresolved: string[]; blocked: { title: string }[] }) {
  const lines = [o.installed.length ? `Installed: ${o.installed.join(", ")}.` : "Nothing new was installed (it may already be there)."];
  if (o.unresolved.length) lines.push(`Couldn't find a fitting version of: ${o.unresolved.join(", ")}.`);
  if (o.blocked.length)
    lines.push(
      `CurseForge only lets its website hand out: ${o.blocked.map((b) => b.title).join(", ")}. Alloy asks the player to fetch them in its CurseForge window.`,
    );
  return lines.join(" ");
}

async function contentTitles(instanceId: string, paths: string[]) {
  const ov = await api.overview(instanceId);
  const byPath = new Map(ov.items.map((x) => [x.path, x]));
  const missing = paths.filter((p) => !byPath.has(p));
  if (missing.length) throw `Not in this instance: ${missing.join(", ")}. Use the paths get_instance lists.`;
  return paths.map((p) => byPath.get(p)!.title);
}

function listed(titles: string[]) {
  return titles.length <= 2 ? titles.join(" and ") : `${titles[0]} and ${titles.length - 1} more`;
}

const changers: Record<string, { plan: (i: any) => Promise<Plan> }> = {
  create_instance: {
    plan: async (i) => {
      const loader = str(i.loader) as Loader;
      const game = str(i.game_version);
      const versions = await api.gameVersions(true);
      if (!versions.some((v) => v.id === game))
        throw `Minecraft ${game} doesn't exist. Newest releases: ${versions
          .filter((v) => v.type === "release")
          .slice(0, 8)
          .map((v) => v.id)
          .join(", ")}.`;
      let lv: string | null = null;
      if (loader !== "vanilla") {
        const lvs = await api.loaderVersions(loader, game);
        if (lvs.length === 0) throw `${loaderName[loader]} has no build for Minecraft ${game}.`;
        lv = i.loader_version
          ? (lvs.find((v) => v.id === i.loader_version)?.id ?? null)
          : (lvs.find((v) => v.recommended) ?? lvs.find((v) => v.stable) ?? lvs[0]).id;
        if (!lv) throw `${loaderName[loader]} ${i.loader_version} doesn't exist for Minecraft ${game}.`;
      }
      const name = str(i.name).trim() || `${loaderName[loader]} ${game}`;
      return {
        title: `Create “${name}”`,
        detail: `Minecraft ${game}, ${loaderName[loader]}${lv ? ` ${lv}` : ""}${loader === "fabric" || loader === "quilt" ? ", with Fabric API" : ""}`,
        noCheckpoint: true,
        run: async () => {
          const inst = await api.createInstance({
            name,
            gameVersion: game,
            loader,
            loaderVersion: lv,
            fabricApi: true,
          });
          return `Created "${inst.name}" with instance_id ${inst.id}. Alloy is downloading the game files in the background; content can be installed right away.`;
        },
      };
    },
  },
  install_content: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      if (i.platform === "curseforge") {
        const modId = Number(i.project_id);
        const [m, files] = await Promise.all([api.curseforgeProject(modId), api.curseforgeFiles(modId, inst.id)]);
        const file = i.version_id ? files.find((f) => String(f.id) === String(i.version_id)) : (files.find((f) => f.releaseType === 1) ?? files[0]);
        if (!file) throw `No file of ${m.name} runs on Minecraft ${inst.gameVersion} with ${loaderName[inst.loader]}.`;
        return {
          title: `Install ${m.name} into ${inst.name}`,
          detail: `${file.displayName} from CurseForge, with what it requires`,
          instanceId: inst.id,
          run: async () => outcomeText(await api.installCurseForgeFile(inst.id, modId, file.id, m.name)),
        };
      }
      const p = await api.modrinthProject(str(i.project_id));
      const versions = await api.modrinthVersions(p.id, inst.id);
      const v = i.version_id ? versions.find((x) => x.id === i.version_id) : (versions.find((x) => x.version_type === "release") ?? versions[0]);
      if (!v) throw `No version of ${p.title} runs on Minecraft ${inst.gameVersion} with ${loaderName[inst.loader]}.`;
      return {
        title: `Install ${p.title} into ${inst.name}`,
        detail: `${v.version_number} from Modrinth, with what it requires`,
        instanceId: inst.id,
        run: async () => outcomeText(await api.installModrinthVersion(inst.id, v.id, p.title)),
      };
    },
  },
  propose_pack: {
    plan: async (i) => {
      const groups: PackProposal["groups"] = (Array.isArray(i.groups) ? i.groups : []).map((g: any, gi: number) => ({
        name: str(g.name) || `Group ${gi + 1}`,
        items: (Array.isArray(g.items) ? g.items : []).map((x: any) => ({
          key: planKey(x.platform, x.project_id),
          platform: x.platform === "curseforge" ? "curseforge" : "modrinth",
          projectId: str(x.project_id),
          title: str(x.title) || str(x.project_id),
          why: str(x.why) || undefined,
        })),
      }));
      const proposal: PackProposal = {
        name: str(i.name) || "New pack",
        gameVersion: str(i.game_version),
        loader: str(i.loader),
        summary: str(i.summary),
        groups,
      };
      const all = groups.flatMap((g) => g.items);
      return {
        title: `Plan for ${proposal.name}`,
        proposal,
        items: all.map((x) => ({ key: x.key, title: x.title, detail: x.why })),
        run: async (selected, progress, note) => {
          const keep = new Set(selected ?? all.map((x) => x.key));
          const removed = all.filter((x) => !keep.has(x.key)).map((x) => x.title);
          useAssistant.setState({
            plan: { name: proposal.name, keys: [...keep] },
          });
          progress?.(`Approved the plan for ${proposal.name}: ${keep.size} mods`);
          const lines = [
            `The player approved the plan "${proposal.name}" with ${keep.size} of ${all.length} mods. Installs with install_contents from it won't ask again.`,
          ];
          if (removed.length) lines.push(`They removed: ${removed.join(", ")}. Don't add those.`);
          if (note?.trim()) lines.push(`Their note: ${note.trim()}`);
          return lines.join("\n");
        },
      };
    },
  },
  find_culprit: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      if (useStore.getState().game[inst.id]) throw `${inst.name} is running. Ask the player to close it first.`;
      return {
        title: `Find the mod that crashes ${inst.name}`,
        detail:
          "Alloy starts the game several times with halves of the mods off until one is left, then puts everything back. This takes a few minutes, more for big packs.",
        instanceId: inst.id,
        noCheckpoint: true,
        run: async () => {
          const f = await api.bisectStart(inst.id);
          if (f.names.length === 0) return `Found nothing: ${inst.name} started fine with every half, so the crash may depend on the world or on timing.`;
          return `${f.alone ? "Found it" : "Narrowed it down"} after ${f.trials} starts: ${f.names.join(", ")} (${f.culprits.join(", ")}).${f.alone ? "" : " These crash only together."} All mods are back as they were.`;
        },
      };
    },
  },
  make_server_pack: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      return {
        title: `Make a server from ${inst.name}`,
        detail: "You'll pick an empty folder. The server gets the same Minecraft and loader, the mods without client-only ones, the configs and start scripts.",
        noCheckpoint: true,
        run: async () => {
          const dest = isTauri ? await openFolderDialog({ directory: true, title: "Choose an empty folder for the server" }) : "~/Servers/demo";
          if (typeof dest !== "string") return "The player didn't pick a folder, so no server was made.";
          const r = await api.makeServerPack(inst.id, dest, false);
          return [
            `Made a server in ${r.dir} with ${r.mods} mods. Start it with ${r.start}; it needs Java ${r.javaMajor}.`,
            r.leftOut.length ? `Left out as client-only: ${r.leftOut.join(", ")}.` : "No mods were client-only.",
            "eula.txt says eula=false: the player must accept Minecraft's EULA there before the first start.",
          ].join(" ");
        },
      };
    },
  },
  install_contents: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      const wanted: {
        platform?: string;
        project_id?: string;
        version_id?: string;
        why?: string;
      }[] = Array.isArray(i.items) ? i.items.slice(0, 40) : [];
      if (wanted.length === 0) throw "No items to install.";
      const ov = await api.overview(inst.id).catch(() => null);
      const have = new Set((ov?.items ?? []).map((x) => x.meta?.projectId).filter(Boolean) as string[]);
      // Look every item up first (a few at a time) so the card shows real names and versions.
      type Resolved = BatchItem & {
        install?: () => Promise<{
          installed: string[];
          unresolved: string[];
          blocked: { title: string }[];
        }>;
      };
      const resolve = async (w: (typeof wanted)[number], n: number): Promise<Resolved> => {
        const key = `${n}`;
        const id = str(w.project_id);
        try {
          if (w.platform === "curseforge") {
            const modId = Number(id);
            const [m, files] = await Promise.all([api.curseforgeProject(modId), api.curseforgeFiles(modId, inst.id)]);
            if (have.has(String(m.id))) return { key, title: m.name, unavailable: "already installed" };
            const f = w.version_id ? files.find((x) => String(x.id) === String(w.version_id)) : (files.find((x) => x.releaseType === 1) ?? files[0]);
            if (!f)
              return {
                key,
                title: m.name,
                unavailable: `nothing for ${inst.gameVersion} ${loaderName[inst.loader]}`,
              };
            return {
              key,
              title: m.name,
              detail: w.why || f.displayName,
              install: () => api.installCurseForgeFile(inst.id, modId, f.id, m.name),
            };
          }
          const p = await api.modrinthProject(id);
          if (have.has(p.id)) return { key, title: p.title, unavailable: "already installed" };
          const versions = await api.modrinthVersions(p.id, inst.id);
          const v = w.version_id ? versions.find((x) => x.id === w.version_id) : (versions.find((x) => x.version_type === "release") ?? versions[0]);
          if (!v)
            return {
              key,
              title: p.title,
              unavailable: `nothing for ${inst.gameVersion} ${loaderName[inst.loader]}`,
            };
          return {
            key,
            title: p.title,
            detail: w.why || v.version_number,
            install: () => api.installModrinthVersion(inst.id, v.id, p.title),
          };
        } catch {
          return {
            key,
            title: id || "Unknown project",
            unavailable: "not found",
          };
        }
      };
      const resolved: Resolved[] = new Array(wanted.length);
      let next = 0;
      await Promise.all(
        Array.from({ length: Math.min(6, wanted.length) }, async () => {
          while (next < wanted.length) {
            const n = next++;
            resolved[n] = await resolve(wanted[n], n);
          }
        }),
      );
      const ok = resolved.filter((r) => !r.unavailable);
      if (ok.length === 0) throw new Nothing(`Nothing to install: ${resolved.map((r) => `${r.title} (${r.unavailable})`).join(", ")}.`);
      const approved = new Set(useAssistant.getState().plan?.keys ?? []);
      const fromPlan = approved.size > 0 && wanted.every((w) => approved.has(planKey(w.platform, w.project_id)));
      return {
        preApproved: fromPlan,
        title: `Install ${ok.length} thing${ok.length > 1 ? "s" : ""} into ${inst.name}`,
        detail: "Each comes with what it requires. Untick anything you don't want.",
        items: resolved.map(({ install: _i, ...rest }) => rest),
        instanceId: inst.id,
        run: async (selected, progress) => {
          const keep = new Set(selected ?? ok.map((r) => r.key));
          const todo = resolved.filter((r) => r.install && keep.has(r.key));
          const installed: string[] = [],
            failed: string[] = [],
            blocked: string[] = [],
            unresolved: string[] = [];
          for (const [n, r] of todo.entries()) {
            progress?.(`Installing ${n + 1} of ${todo.length} into ${inst.name}: ${r.title}`);
            try {
              const out = await r.install!();
              installed.push(...out.installed);
              unresolved.push(...out.unresolved);
              blocked.push(...out.blocked.map((b) => b.title));
            } catch (e) {
              failed.push(`${r.title} (${String(e)})`);
            }
          }
          progress?.(`Installed ${todo.length - failed.length} of ${todo.length} into ${inst.name}`);
          const skipped = resolved.filter((r) => r.install && !keep.has(r.key)).map((r) => r.title);
          const lines = [installed.length ? `Installed: ${[...new Set(installed)].join(", ")}.` : "Nothing new was installed."];
          if (skipped.length) lines.push(`The player unticked: ${skipped.join(", ")}.`);
          const na = resolved.filter((r) => r.unavailable);
          if (na.length) lines.push(`Not installed: ${na.map((r) => `${r.title} (${r.unavailable})`).join(", ")}.`);
          if (unresolved.length) lines.push(`Couldn't find a fitting version of: ${[...new Set(unresolved)].join(", ")}.`);
          if (failed.length) lines.push(`Failed: ${failed.join("; ")}.`);
          if (blocked.length)
            lines.push(`CurseForge only lets its website hand out: ${blocked.join(", ")}. Alloy asks the player to fetch them in its CurseForge window.`);
          return lines.join(" ");
        },
      };
    },
  },
  set_enabled: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      const paths: string[] = Array.isArray(i.paths) ? i.paths : [];
      const titles = await contentTitles(inst.id, paths);
      const on = !!i.enabled;
      return {
        title: `Turn ${on ? "on" : "off"} ${listed(titles)} in ${inst.name}`,
        detail: titles.length > 2 ? titles.join(", ") : undefined,
        instanceId: inst.id,
        run: async () => {
          await api.setContentEnabled(inst.id, paths, on);
          return `Turned ${on ? "on" : "off"}: ${titles.join(", ")}.`;
        },
      };
    },
  },
  remove_content: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      const paths: string[] = Array.isArray(i.paths) ? i.paths : [];
      const titles = await contentTitles(inst.id, paths);
      return {
        title: `Delete ${listed(titles)} from ${inst.name}`,
        detail: titles.length > 2 ? titles.join(", ") : "The files are deleted. A save point is made first.",
        instanceId: inst.id,
        run: async () => {
          await api.removeContent(inst.id, paths);
          return `Deleted: ${titles.join(", ")}.`;
        },
      };
    },
  },
  apply_fix: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      const fixes = await api.fixes(inst.id);
      const f = fixes[Number(i.fix_index)];
      if (!f) throw `There's no fix ${i.fix_index}. get_instance lists ${fixes.length} fix(es) now.`;
      return {
        title: `${f.label} in ${inst.name}`,
        detail: f.message,
        instanceId: inst.id,
        run: async () => {
          const out = await api.applyFix(inst.id, f.fix);
          return `Applied: ${f.label}. ${f.fix.action.startsWith("install") ? outcomeText(out) : ""}`.trim();
        },
      };
    },
  },
  update_content: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      const ups = await api.checkContentUpdates(inst.id);
      if (ups.length === 0) throw new Nothing(`Everything in ${inst.name} is up to date.`);
      return {
        title: `Update ${ups.length} thing${ups.length > 1 ? "s" : ""} in ${inst.name}`,
        detail: ups.map((u) => `${u.title} ${u.current ?? ""} → ${u.versionName}`).join("\n"),
        instanceId: inst.id,
        noCheckpoint: true, // updating makes its own
        run: async () => {
          const r = await api.applyContentUpdates(inst.id, ups);
          const lines = [r.updated.length ? `Updated: ${r.updated.join(", ")}.` : "Nothing was updated."];
          if (r.heldBack.length) lines.push(`Held back (would break something): ${r.heldBack.map(([a, b]) => `${a} (${b})`).join("; ")}.`);
          if (r.failed.length) lines.push(`Failed: ${r.failed.map(([a, b]) => `${a} (${b})`).join("; ")}.`);
          return lines.join(" ");
        },
      };
    },
  },
  write_file: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      const path = str(i.path);
      const content = str(i.content);
      const before = await api.assistantCurrentText(inst.id, path);
      if (before === content) throw new Nothing(`${path} already says exactly that.`);
      return {
        title: `${before == null ? "Create" : "Change"} ${path} in ${inst.name}`,
        diff: diffLines(before ?? "", content),
        instanceId: inst.id,
        run: async () => {
          await api.assistantWriteFile(inst.id, path, content);
          return `Wrote ${path}.`;
        },
      };
    },
  },
  set_memory: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      const mb = Math.round(Number(i.mb));
      const advice = await api.memoryAdvice(inst.id).catch(() => null);
      if (advice && mb > advice.maxGameMb) throw `This computer can give a game at most ${gb(advice.maxGameMb)}.`;
      const settings = await api.getSettings();
      const now = inst.memoryMb ?? settings.defaultMemoryMb;
      if (now === mb) throw new Nothing(`${inst.name} already has ${gb(mb)}.`);
      return {
        title: `Give ${inst.name} ${gb(mb)} of memory`,
        detail: `It has ${gb(now)} now.`,
        instanceId: inst.id,
        noCheckpoint: true,
        run: async () => {
          const fresh = await api.instance(inst.id);
          await api.updateInstance({ ...fresh, memoryMb: mb });
          return `${inst.name} now gets ${gb(mb)}.`;
        },
      };
    },
  },
  test_launch: {
    plan: async (i) => {
      const inst = await instanceNamed(str(i.instance_id));
      if (useStore.getState().game[inst.id]) throw `${inst.name} is running. Ask the player to close it first.`;
      return {
        title: `Start ${inst.name} once to test it`,
        detail: "Alloy starts the game with your account, waits for the title screen, then closes it. Big packs take a few minutes.",
        instanceId: inst.id,
        noCheckpoint: true,
        run: async () => {
          const r = await api.testLaunch(inst.id);
          if (r.started) return `${inst.name} reached the title screen and was closed again. It starts fine.`;
          return `${inst.name} crashed before the title screen.\n${r.diagnosis ? diagnosisText(r.diagnosis) : "The crash doctor found nothing to go on; read_log has the game's output."}`;
        },
      };
    },
  },
};

export const changingTools = new Set(Object.keys(changers));

/** Every tool this Alloy can run, so AlloyMC offers Claude exactly those. */
const toolNames = () => [...Object.keys(readers), ...Object.keys(changers)];

// ---------- diff ----------

export type DiffLine = { kind: "same" | "add" | "del"; text: string } | { kind: "gap"; count: number };

/** A line diff for showing a config change, with unchanged runs folded to a little context. */
export function diffLines(a: string, b: string, context = 2): DiffLine[] {
  const x = a.split("\n");
  const y = b.split("\n");
  // Trim the common start and end, then LCS on what's left (configs are small).
  let start = 0;
  while (start < x.length && start < y.length && x[start] === y[start]) start++;
  let endX = x.length,
    endY = y.length;
  while (endX > start && endY > start && x[endX - 1] === y[endY - 1]) (endX--, endY--);
  const mx = x.slice(start, endX),
    my = y.slice(start, endY);
  const raw: { kind: "same" | "add" | "del"; text: string }[] = x.slice(0, start).map((text) => ({ kind: "same" as const, text }));
  if (mx.length * my.length > 4_000_000) {
    raw.push(...mx.map((text) => ({ kind: "del" as const, text })), ...my.map((text) => ({ kind: "add" as const, text })));
  } else {
    const n = mx.length,
      m = my.length;
    const dp: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = mx[i] === my[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    let i = 0,
      j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && mx[i] === my[j]) (raw.push({ kind: "same", text: mx[i] }), i++, j++);
      else if (i < n && (j === m || dp[i + 1][j] >= dp[i][j + 1])) (raw.push({ kind: "del", text: mx[i] }), i++);
      else (raw.push({ kind: "add", text: my[j] }), j++);
    }
  }
  raw.push(...x.slice(endX).map((text) => ({ kind: "same" as const, text })));
  // Fold unchanged runs.
  const keep = raw.map((l, idx) => l.kind !== "same" || raw.slice(Math.max(0, idx - context), idx + context + 1).some((o) => o.kind !== "same"));
  const out: DiffLine[] = [];
  let gap = 0;
  raw.forEach((l, idx) => {
    if (keep[idx]) {
      if (gap) out.push({ kind: "gap", count: gap });
      gap = 0;
      out.push(l);
    } else gap++;
  });
  if (gap) out.push({ kind: "gap", count: gap });
  return out;
}

/** Text the player wrote in a user message (context and tool results left out). */
export function userText(m: Msg) {
  return m.content
    .filter((b): b is Extract<Block, { type: "text" }> => b.type === "text" && !b.text.startsWith(CONTEXT_TAG))
    .map((b) => b.text)
    .join("\n\n");
}

// Development: the tools, for trying them from the console against the mock or the app.
if (import.meta.env.DEV) (globalThis as any).alloyAssistantTools = { readers, changers };

/** The crash doctor's plain-words explanation: a short side conversation with read-only tools. */
export async function explainCrash(instanceId: string, onText: (text: string) => void, signal: { stopped: boolean }): Promise<void> {
  const inst = await api.instance(instanceId);
  const tools = ["read_crash", "read_log", "get_instance", "read_file", "get_project"];
  const messages: Msg[] = [
    {
      role: "user",
      content: [
        {
          type: "text",
          text: `${CONTEXT_TAG}This is Alloy's crash doctor asking for the player. Instance: "${inst.name}" (instance_id ${inst.id}, Minecraft ${inst.gameVersion}, ${loaderName[inst.loader]}). Answer in plain words for a player, 3 to 6 short sentences: what went wrong, which mod (if any), and what to do. Don't change anything; the doctor's buttons do that.</alloy-context>Why did my game crash?`,
        },
      ],
    },
  ];
  let text = "";
  for (let round = 0; round < 6 && !signal.stopped; round++) {
    const id = newJobId();
    sideStreams.set(id, (d) => {
      if (d.type === "text") {
        text += d.text;
        onText(text);
      }
    });
    try {
      const reply = await api.assistantSend(id, "fast", messages, tools);
      messages.push({ role: "assistant", content: reply.content });
      const uses = reply.content.filter((b): b is ToolUse => b.type === "tool_use");
      if (reply.stopReason !== "tool_use" || uses.length === 0) return;
      const results: Block[] = [];
      for (const u of uses) {
        const r = readers[u.name];
        let content: string;
        try {
          content = r ? cut(await r.run(u.input)) : "Not available here.";
        } catch (e) {
          content = String(e);
        }
        results.push({ type: "tool_result", tool_use_id: u.id, content });
      }
      messages.push({ role: "user", content: results });
      // Text before a tool call was thinking aloud; the answer comes after.
      text = "";
      onText("");
    } finally {
      sideStreams.delete(id);
    }
  }
}

/** The Add button on a project card: the newest version that fits, with what it requires. */
export async function quickInstall(platform: "modrinth" | "curseforge", projectId: string, instanceId: string, title: string) {
  const inst = await api.instance(instanceId);
  if (platform === "curseforge") {
    const files = await api.curseforgeFiles(Number(projectId), inst.id);
    const f = files.find((x) => x.releaseType === 1) ?? files[0];
    if (!f) throw `Nothing of ${title} runs on Minecraft ${inst.gameVersion} with ${loaderName[inst.loader]}.`;
    return api.installCurseForgeFile(inst.id, Number(projectId), f.id, title);
  }
  const versions = await api.modrinthVersions(projectId, inst.id);
  const v = versions.find((x) => x.version_type === "release") ?? versions[0];
  if (!v) throw `Nothing of ${title} runs on Minecraft ${inst.gameVersion} with ${loaderName[inst.loader]}.`;
  return api.installModrinthVersion(inst.id, v.id, title);
}
