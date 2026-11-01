// The assistant panel: a conversation with Claude beside whatever page is open. Claude looks
// at instances, logs and crashes with tools that run in Alloy, and asks before it changes
// anything.

import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import {
  ArrowUp,
  Ban,
  BookOpen,
  Brain,
  Check,
  ChevronDown,
  ChevronLeft,
  Gauge,
  History,
  LogIn,
  Package,
  Sparkles,
  Square,
  SquarePen,
  Trash2,
  TriangleAlert,
  Wrench,
  X,
} from "lucide-react";
import { api } from "../lib/api";
import { useAssistant, userText, type Approval, type AssistantStatus, type BatchItem, type Block, type DiffLine, type Msg } from "../lib/assistant";
import { openExternal } from "../lib/menus";
import { useInstance } from "../lib/queries";
import { ago } from "../lib/format";
import { toastError, useStore } from "../lib/store";
import { Ori } from "./ori/Ori";
import { PlanCard, ProjectCards, UndoBar } from "./AssistantCards";

const WIDTH = 420;

/** The rail button that opens and closes the panel. */
export function AssistantButton() {
  const open = useAssistant((s) => s.open);
  const busy = useAssistant((s) => s.busy);
  const waiting = useAssistant((s) => !!s.approval);
  return (
    <button
      onClick={() => useAssistant.getState().setOpen(!open)}
      aria-label="Assistant"
      aria-pressed={open}
      className={`group relative grid h-11 w-11 place-items-center rounded-[8px] transition-[color,background,transform] duration-150 active:scale-90 ${
        open ? "bg-slate-800 text-gold" : "text-ink-3 hover:bg-slate-850 hover:text-ink"
      }`}
    >
      <Sparkles
        size={19}
        strokeWidth={open ? 2.2 : 1.8}
        className={`relative transition-transform duration-200 group-hover:scale-110 ${busy ? "animate-pulse" : ""}`}
      />
      {waiting && !open && <span className="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-gold" />}
      <span className="pointer-events-none absolute left-[52px] z-50 -translate-x-1 whitespace-nowrap rounded-[6px] border border-line bg-slate-800 px-2 py-1 text-[12px] text-ink opacity-0 shadow-lg transition-[opacity,transform] duration-150 group-hover:translate-x-0 group-hover:opacity-100">
        Assistant
      </span>
    </button>
  );
}

/** Follows the page: on an instance page the conversation is about that instance. */
function useFollowPage() {
  const page = useStore((s) => s.page);
  useEffect(() => {
    const a = useAssistant.getState();
    if (a.messages.length > 0 || a.busy) return;
    a.setInstance(page.name === "instance" ? page.id : null);
  }, [page]);
}

export function AssistantPanel() {
  useFollowPage();
  const open = useAssistant((s) => s.open);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "j") {
        e.preventDefault();
        const a = useAssistant.getState();
        a.setOpen(!a.open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <AnimatePresence initial={false}>
      {open && (
        <motion.aside
          key="assistant"
          initial={{ width: 0, opacity: 0 }}
          animate={{ width: WIDTH, opacity: 1 }}
          exit={{ width: 0, opacity: 0 }}
          transition={{ duration: 0.24, ease: [0.22, 1, 0.36, 1] }}
          className="relative flex shrink-0 overflow-hidden border-l border-line-soft/60 bg-slate-900/92"
          aria-label="Assistant"
        >
          <div className="flex h-full flex-col" style={{ width: WIDTH }}>
            <Panel />
          </div>
        </motion.aside>
      )}
    </AnimatePresence>
  );
}

function Panel() {
  const [view, setView] = useState<"chat" | "history">("chat");
  const status = useQuery({
    queryKey: ["assistant", "status"],
    queryFn: api.assistantStatus,
    staleTime: 30_000,
  });
  const busy = useAssistant((s) => s.busy);
  const qc = useQueryClient();
  // Usage moves with every reply.
  useEffect(() => {
    if (!busy) qc.invalidateQueries({ queryKey: ["assistant", "status"] });
  }, [busy, qc]);

  return (
    <>
      <Header view={view} setView={setView} />
      {view === "history" ? (
        <HistoryList onPick={() => setView("chat")} />
      ) : (
        <>
          <Conversation />
          <Footer status={status.data} loading={status.isLoading} />
        </>
      )}
    </>
  );
}

function Header({ view, setView }: { view: "chat" | "history"; setView: (v: "chat" | "history") => void }) {
  const hasMessages = useAssistant((s) => s.messages.length > 0);
  const instanceId = useAssistant((s) => s.instanceId);
  const inst = useInstance(instanceId ?? "");
  const history = view === "history";
  return (
    <div className="flex h-[52px] shrink-0 items-center gap-2.5 border-b border-line-soft/70 pl-3.5 pr-2.5">
      {history ? (
        <button className="ai-icon -ml-1" aria-label="Back to the conversation" onClick={() => setView("chat")}>
          <ChevronLeft size={17} />
        </button>
      ) : (
        <span className="ai-spark" aria-hidden>
          <Sparkles size={14} strokeWidth={2.2} />
        </span>
      )}
      <div className="min-w-0 flex-1">
        <p className="text-[13.5px] font-semibold leading-[1.2] text-ink">{history ? "Conversations" : "Assistant"}</p>
        {!history && <p className="truncate text-[11.5px] leading-[1.3] text-ink-3">{inst.data ? `About ${inst.data.name}` : "Claude, inside Alloy"}</p>}
      </div>
      <div className="flex items-center gap-0.5">
        {!history && hasMessages && (
          <button className="ai-icon" aria-label="New conversation" title="New conversation" onClick={() => useAssistant.getState().reset()}>
            <SquarePen size={15} />
          </button>
        )}
        {!history && (
          <button className="ai-icon" aria-label="Earlier conversations" title="Earlier conversations" onClick={() => setView("history")}>
            <History size={15} />
          </button>
        )}
        <button className="ai-icon" aria-label="Close the assistant" title="Close (Ctrl+J)" onClick={() => useAssistant.getState().setOpen(false)}>
          <X size={16} />
        </button>
      </div>
    </div>
  );
}

// ---------- conversation ----------

function Conversation() {
  const messages = useAssistant((s) => s.messages);
  const live = useAssistant((s) => s.live);
  const busy = useAssistant((s) => s.busy);
  const error = useAssistant((s) => s.error);
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  // Undo sits after the last message of the reply that made the changes.
  const undo = useAssistant((s) => s.undo);
  const undoAt = useMemo(() => {
    const at = new Map<number, string>();
    for (const k of Object.keys(undo)) {
      const start = Number(k);
      let end = messages.length - 1;
      for (let j = start + 1; j < messages.length; j++) {
        if (messages[j].role === "user" && userText(messages[j])) {
          end = j - 1;
          break;
        }
      }
      at.set(end, k);
    }
    return at;
  }, [undo, messages]);

  // Results by tool use id, to show how each tool call went.
  const results = useMemo(() => {
    const m = new Map<string, Extract<Block, { type: "tool_result" }>>();
    for (const msg of messages) for (const b of msg.content) if (b.type === "tool_result") m.set(b.tool_use_id, b);
    return m;
  }, [messages]);

  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  });

  return (
    <div
      ref={ref}
      className="min-h-0 flex-1 overflow-y-auto px-4 py-4"
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
      }}
    >
      {messages.length === 0 && !busy ? (
        <Welcome />
      ) : (
        <div className="flex flex-col gap-3">
          {messages.map((m, i) => (
            <Fragment key={i}>
              <MessageView m={m} results={results} />
              {undoAt.has(i) && !(busy && i >= messages.length - 2) && <UndoBar k={undoAt.get(i)!} entry={undo[undoAt.get(i)!]} />}
            </Fragment>
          ))}
          {live && <LiveReply blocks={live.blocks} />}
          {error && <p className="rounded-[8px] border border-redstone/30 bg-redstone/10 px-3 py-2 text-[13px] text-ink-2">{error}</p>}
        </div>
      )}
    </div>
  );
}

const ideas: { text: string; icon: typeof Sparkles }[] = [
  { text: "Why does my game crash?", icon: TriangleAlert },
  {
    text: "Build me a cozy 1.21.1 pack with Create and shaders",
    icon: Package,
  },
  { text: "Make this instance run faster", icon: Gauge },
  { text: "What does each mod in this pack do?", icon: BookOpen },
];

function Welcome() {
  const send = useAssistant((s) => s.send);
  return (
    <div className="flex flex-col pt-4">
      <div className="flex flex-col items-center text-center">
        <Ori pose="hi" size={96} />
        <h3 className="mt-2 font-pixel text-[19px] leading-tight text-ink">Ask about your Minecraft</h3>
        <p className="mt-1.5 max-w-[290px] text-[13px] leading-relaxed text-ink-3">
          Claude looks at your instances, logs and crashes, finds mods and fixes things. It asks before it changes anything.
        </p>
      </div>
      <ul className="mt-6 overflow-hidden rounded-[12px] border border-line-soft bg-slate-850/50">
        {ideas.map(({ text, icon: Icon }) => (
          <li key={text} className="border-b border-line-soft last:border-b-0">
            <button
              className="group flex w-full items-center gap-3 px-3.5 py-2.5 text-left text-[13px] text-ink-2 transition-colors hover:bg-slate-800/70 hover:text-ink"
              onClick={() => void send(text)}
            >
              <Icon size={15} className="shrink-0 text-ink-3 transition-colors group-hover:text-gold" />
              <span className="min-w-0 flex-1">{text}</span>
              <ArrowUp size={14} className="shrink-0 rotate-45 text-gold opacity-0 transition-opacity group-hover:opacity-100" />
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function MessageView({ m, results }: { m: Msg; results: Map<string, Extract<Block, { type: "tool_result" }>> }) {
  if (m.role === "user") {
    const text = userText(m);
    if (!text) return null;
    return (
      <div className="ml-10 self-end whitespace-pre-wrap rounded-[12px] rounded-br-[4px] bg-slate-800 px-3 py-2 text-[13.5px] text-ink" data-selectable>
        {text}
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      {m.content.map((b, i) =>
        b.type === "text" ? (
          <Markdown key={i} text={b.text} />
        ) : b.type === "tool_use" ? (
          <ToolRow key={i} id={b.id} name={b.name} input={b.input} result={results.get(b.id)} />
        ) : null,
      )}
    </div>
  );
}

function LiveReply({ blocks }: { blocks: Record<number, { text?: string; tool?: string }> }) {
  const keys = Object.keys(blocks)
    .map(Number)
    .sort((a, b) => a - b);
  if (keys.length === 0) return <Working />;
  return (
    <div className="flex flex-col gap-2">
      {keys.map((k) =>
        blocks[k].tool ? <ToolRow key={k} id={`live-${k}`} name={blocks[k].tool!} pending /> : <Markdown key={k} text={blocks[k].text ?? ""} />,
      )}
    </div>
  );
}

function Working() {
  return (
    <div className="flex items-center gap-2 text-[12.5px] text-ink-3">
      <span className="spinner" /> Thinking
    </div>
  );
}

const verbs: Record<string, string> = {
  list_instances: "Looking at your instances",
  get_instance: "Checking the instance",
  search_projects: "Searching",
  get_project: "Reading a project page",
  read_log: "Reading the log",
  read_crash: "Reading the crash",
  read_file: "Reading a file",
  create_instance: "Getting a new instance ready",
  install_content: "Getting an install ready",
  set_enabled: "Getting a change ready",
  remove_content: "Getting a change ready",
  apply_fix: "Getting a fix ready",
  update_content: "Checking for updates",
  write_file: "Writing a config change",
  set_memory: "Getting a change ready",
  test_launch: "Getting a test start ready",
};

function ToolRow({
  id,
  name,
  input,
  result,
  pending,
}: {
  id: string;
  name: string;
  input?: Record<string, any>;
  result?: Extract<Block, { type: "tool_result" }>;
  pending?: boolean;
}) {
  const label = useAssistant((s) => s.labels[id]);
  const running = useAssistant((s) => s.running === id);
  const approval = useAssistant((s) => (s.approval?.toolUseId === id ? s.approval : null));
  const [open, setOpen] = useState(false);
  if (approval?.proposal) return <PlanCard p={approval.proposal} />;
  if (approval) return <ApprovalCard a={approval} />;
  if (name === "show_projects" && input && result && !result.is_error) return <ProjectCards input={input} />;
  const declined = result?.is_error && /declined|Stopped by the player/.test(result.content);
  const failed = result?.is_error && !declined;
  const icon =
    pending || running || !result ? (
      <span className="spinner" />
    ) : declined ? (
      <Ban size={13} className="text-ink-3" />
    ) : failed ? (
      <X size={13} className="text-redstone" />
    ) : (
      <Check size={13} className="text-grass" />
    );
  return (
    <div className="text-[12.5px]">
      <button
        className="flex w-full items-center gap-2 rounded-[6px] px-1 py-0.5 text-left text-ink-3 transition-colors hover:text-ink-2 disabled:hover:text-ink-3"
        onClick={() => setOpen(!open)}
        disabled={!result}
        aria-expanded={open}
      >
        <span className="grid w-4 shrink-0 place-items-center">{icon}</span>
        <span className="min-w-0 flex-1 truncate">{label ?? verbs[name] ?? name}</span>
        {result && <ChevronDown size={13} className={`shrink-0 transition-transform ${open ? "rotate-180" : ""}`} />}
      </button>
      <AnimatePresence initial={false}>
        {open && result && (
          <motion.pre
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="mt-1 max-h-60 overflow-auto whitespace-pre-wrap rounded-[6px] bg-slate-950/80 px-2.5 py-2 font-mono text-[11px] leading-relaxed text-ink-3"
            data-selectable
          >
            {result.content}
          </motion.pre>
        )}
      </AnimatePresence>
    </div>
  );
}

// Lucide's copy and check, inline: the buttons are added to rendered HTML, outside React.
const COPY_ICONS =
  '<svg class="i-copy" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>' +
  '<svg class="i-done" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>';

export function Markdown({ text }: { text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const html = useMemo(() => DOMPurify.sanitize(marked.parse(text, { async: false, breaks: true }) as string), [text]);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      const copy = target.closest<HTMLButtonElement>("button.code-copy");
      if (copy) {
        const code = copy.parentElement?.querySelector("code")?.textContent ?? "";
        void navigator.clipboard.writeText(code.replace(/\n$/, "")).then(() => {
          copy.dataset.done = "";
          copy.setAttribute("aria-label", "Copied");
          setTimeout(() => {
            delete copy.dataset.done;
            copy.setAttribute("aria-label", "Copy");
          }, 1400);
        });
        return;
      }
      const a = target.closest("a");
      if (a?.href) {
        e.preventDefault();
        void openExternal(a.href);
      }
    };
    el.addEventListener("click", onClick);
    return () => el.removeEventListener("click", onClick);
  }, []);
  // A copy button on every code block.
  useEffect(() => {
    ref.current?.querySelectorAll("pre").forEach((pre) => {
      if (pre.querySelector(".code-copy")) return;
      const b = document.createElement("button");
      b.className = "code-copy";
      b.type = "button";
      b.setAttribute("aria-label", "Copy");
      b.title = "Copy";
      b.innerHTML = COPY_ICONS;
      pre.appendChild(b);
    });
  }, [html]);
  if (!text.trim()) return null;
  return <div ref={ref} className="prose-alloy prose-chat" data-selectable dangerouslySetInnerHTML={{ __html: html }} />;
}

// ---------- approval, input and status ----------

function Footer({ status, loading }: { status?: AssistantStatus; loading: boolean }) {
  return (
    <div className="shrink-0 px-3 pb-3 pt-1">
      {loading ? null : !status?.signedIn ? <SignInNotice /> : !status.available ? <Unavailable status={status} /> : <Composer status={status} />}
    </div>
  );
}

function Notice({ text, action }: { text: string; action?: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2 rounded-[10px] border border-line-soft bg-slate-850/70 p-3 text-[13px] text-ink-2">
      <p>{text}</p>
      {action}
    </div>
  );
}

function SignInNotice() {
  const [busy, setBusy] = useState(false);
  return (
    <Notice
      text="The assistant comes with your free AlloyMC account. Sign in, confirm your email and link a Minecraft account to use it."
      action={
        <button
          className="btn btn-gold self-start"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await api.alloymcSignIn();
            } catch (e) {
              toastError(e);
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? <span className="spinner" /> : <LogIn size={14} />} Sign in to AlloyMC
        </button>
      }
    />
  );
}

function Unavailable({ status }: { status: AssistantStatus }) {
  if (status.needs === "email")
    return (
      <Notice
        text={status.reason ?? "Confirm your email first."}
        action={
          <button className="btn self-start" onClick={() => openExternal("https://alloymc.dev/settings")}>
            Open alloymc.dev
          </button>
        }
      />
    );
  const resets = status.dayResets ? new Date(status.dayResets) : null;
  const text = status.reason ?? "The assistant isn't available right now.";
  const back = resets && status.dayUsed >= 100 ? ` It's back at ${resets.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}.` : "";
  return <Notice text={text + back} />;
}

/** How much of today's (or this month's, if that's closer) share is used, as a small ring. */
function Usage({ status }: { status: AssistantStatus }) {
  if (status.unlimited) return null;
  const used = Math.min(100, Math.max(status.dayUsed, status.monthUsed));
  const which = status.dayUsed >= status.monthUsed ? "today" : "this month";
  const r = 6;
  const c = 2 * Math.PI * r;
  return (
    <span
      className="flex items-center gap-1.5 text-[11.5px] text-ink-3"
      title={`Used ${status.dayUsed}% of today's share and ${status.monthUsed}% of this month's`}
    >
      <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden>
        <circle cx="8" cy="8" r={r} fill="none" stroke="var(--color-slate-700)" strokeWidth="2.2" />
        <circle
          cx="8"
          cy="8"
          r={r}
          fill="none"
          stroke={used > 85 ? "var(--color-molten)" : "var(--color-gold)"}
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeDasharray={`${(used / 100) * c} ${c}`}
          transform="rotate(-90 8 8)"
        />
      </svg>
      {used}% {which}
    </span>
  );
}

function Composer({ status }: { status: AssistantStatus }) {
  const [text, setText] = useState("");
  const busy = useAssistant((s) => s.busy);
  const mode = useAssistant((s) => s.mode);
  const pending = useAssistant((s) => (s.approval ? (s.approval.proposal ? "plan" : "change") : null));
  const ref = useRef<HTMLTextAreaElement>(null);
  const open = useAssistant((s) => s.open);
  useEffect(() => {
    if (open) ref.current?.focus();
  }, [open]);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(160, el.scrollHeight)}px`;
  }, [text]);
  const ready = !!text.trim() && !busy;
  const submit = () => {
    if (!ready) return;
    void useAssistant.getState().send(text);
    setText("");
  };
  const deep = mode === "deep";
  return (
    <div className="ai-plate" onClick={(e) => e.target === e.currentTarget && ref.current?.focus()}>
      <textarea
        ref={ref}
        rows={1}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            submit();
          }
        }}
        placeholder={pending === "plan" ? "Approve the plan or ask for changes first" : pending ? "Allow or decline the change first" : busy ? "Claude is working…" : "Ask about mods, crashes, packs…"}
        disabled={busy}
        maxLength={8000}
        className="block max-h-40 min-h-[44px] w-full resize-none bg-transparent px-3.5 pb-1 pt-3 text-[13.5px] leading-[1.5] text-ink outline-none placeholder:text-ink-3"
        aria-label="Message"
      />
      <div className="flex items-center gap-2 px-2 pb-2 pt-1">
        <button
          className="ai-chip"
          aria-pressed={deep}
          onClick={() => useAssistant.getState().setMode(deep ? "fast" : "deep")}
          title={deep ? "Claude Sonnet: slower, for knotty crashes and building packs" : "Use Claude Sonnet for harder problems"}
        >
          <Brain size={13} /> Think harder
        </button>
        <span className="flex-1" />
        <Usage status={status} />
        {busy ? (
          <button className="ai-send" aria-label="Stop" onClick={() => useAssistant.getState().stop()}>
            <Square size={10} fill="currentColor" strokeWidth={0} />
          </button>
        ) : (
          <button className="ai-send" data-on={ready ? "" : undefined} aria-label="Send" disabled={!ready} onClick={submit}>
            <ArrowUp size={15} strokeWidth={2.4} />
          </button>
        )}
      </div>
    </div>
  );
}

function ApprovalCard({ a }: { a: Approval }) {
  const answer = useAssistant((s) => s.answer);
  const [picked, setPicked] = useState<Set<string>>(() => new Set((a.items ?? []).filter((i) => !i.unavailable).map((i) => i.key)));
  const batch = !!a.items;
  const chosen = batch ? [...picked] : undefined;
  const toggle = (key: string) =>
    setPicked((p) => {
      const n = new Set(p);
      if (n.has(key)) n.delete(key);
      else n.add(key);
      return n;
    });
  return (
    <motion.div initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} className="ai-ask" role="alertdialog" aria-label="Allow this change?">
      <div className="flex items-start gap-2.5 px-3.5 pt-3">
        <Wrench size={14} className="mt-[3px] shrink-0 text-gold" />
        <div className="min-w-0 flex-1">
          <p className="text-[13.5px] font-medium leading-snug text-ink">{a.title}</p>
          {a.detail && <p className="mt-1 max-h-28 overflow-y-auto whitespace-pre-wrap text-[12.5px] leading-relaxed text-ink-3">{a.detail}</p>}
        </div>
      </div>
      {a.diff && (
        <div className="px-3.5">
          <Diff lines={a.diff} />
        </div>
      )}
      {a.items && <Checklist items={a.items} picked={picked} toggle={toggle} />}
      <div className="mt-3 flex items-center gap-2 border-t border-line-soft/80 px-3.5 py-2.5">
        {!a.always && (
          <button className="mr-auto rounded-[6px] text-[12px] text-ink-3 transition-colors hover:text-ink" onClick={() => answer(true, true, chosen)}>
            Allow the rest of this chat
          </button>
        )}
        {a.always && <span className="mr-auto text-[12px] text-ink-3">Deleting always asks</span>}
        <button className="btn btn-quiet h-8" onClick={() => answer(false)}>
          Decline
        </button>
        <button className="btn btn-gold h-8" disabled={batch && picked.size === 0} onClick={() => answer(true, false, chosen)}>
          <Check size={13} strokeWidth={2.6} /> {batch ? `Install ${picked.size}` : "Allow"}
        </button>
      </div>
    </motion.div>
  );
}

/** A batch's items, each with a tick the player can take away. */
function Checklist({ items, picked, toggle }: { items: BatchItem[]; picked: Set<string>; toggle: (key: string) => void }) {
  const available = items.filter((i) => !i.unavailable);
  const all = available.every((i) => picked.has(i.key));
  return (
    <div className="mt-2.5 px-3.5">
      <div className="mb-1 flex items-center justify-between text-[11.5px] text-ink-3">
        <span>
          {picked.size} of {available.length} ticked
        </span>
        <button
          className="rounded-[4px] transition-colors hover:text-ink"
          onClick={() => available.forEach((i) => (all ? picked.has(i.key) : !picked.has(i.key)) && toggle(i.key))}
        >
          {all ? "Untick all" : "Tick all"}
        </button>
      </div>
      <ul className="max-h-64 overflow-y-auto rounded-[8px] border border-line-soft bg-slate-950/60">
        {items.map((i) => {
          const on = picked.has(i.key);
          return (
            <li key={i.key} className="border-b border-line-soft/70 last:border-b-0">
              <label className={`flex items-start gap-2.5 px-2.5 py-2 ${i.unavailable ? "opacity-55" : "cursor-pointer hover:bg-slate-850"}`}>
                <input type="checkbox" className="ai-check mt-[2px]" checked={on} disabled={!!i.unavailable} onChange={() => toggle(i.key)} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[12.5px] text-ink">{i.title}</span>
                  <span className="block truncate text-[11.5px] text-ink-3">{i.unavailable ? `Skipped: ${i.unavailable}` : i.detail}</span>
                </span>
              </label>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function Diff({ lines }: { lines: DiffLine[] }) {
  return (
    <pre
      className="mt-2.5 max-h-56 overflow-auto rounded-[8px] border border-line-soft bg-slate-950/90 py-1.5 font-mono text-[11px] leading-[1.6]"
      data-selectable
    >
      {lines.map((l, i) =>
        l.kind === "gap" ? (
          <div key={i} className="px-2.5 text-ink-3/70">
            ⋯ {l.count} unchanged line{l.count > 1 ? "s" : ""}
          </div>
        ) : (
          <div
            key={i}
            className={`whitespace-pre-wrap px-2.5 ${l.kind === "add" ? "bg-grass/10 text-grass" : l.kind === "del" ? "bg-redstone/10 text-redstone" : "text-ink-3"}`}
          >
            {l.kind === "add" ? "+ " : l.kind === "del" ? "- " : "  "}
            {l.text || " "}
          </div>
        ),
      )}
    </pre>
  );
}

// ---------- history ----------

/** What Claude keeps about the player between conversations, with a way to delete each. */
function Memory() {
  const { data, refetch } = useQuery({
    queryKey: ["assistant", "memory"],
    queryFn: api.assistantMemory,
  });
  const notes = data ?? [];
  if (notes.length === 0) return null;
  return (
    <section className="mx-2 mb-2 rounded-[12px] border border-line-soft bg-slate-850/50 p-3">
      <h4 className="flex items-center gap-2 text-[12.5px] font-semibold text-ink">
        <Brain size={13} className="text-gold" /> Claude remembers
      </h4>
      <ul className="mt-2 flex flex-col gap-1">
        {notes.map((n, i) => (
          <li key={`${i}-${n}`} className="group flex items-start gap-2 rounded-[6px] py-0.5 text-[12.5px] leading-snug text-ink-2">
            <span className="min-w-0 flex-1">{n}</span>
            <button
              className="shrink-0 rounded-[4px] p-0.5 text-ink-3 opacity-0 transition-opacity hover:text-ink group-hover:opacity-100 focus-visible:opacity-100"
              aria-label={`Forget “${n}”`}
              onClick={async () => {
                await api.assistantMemorySet(notes.filter((_, k) => k !== i)).catch(toastError);
                void refetch();
              }}
            >
              <X size={13} />
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function HistoryList({ onPick }: { onPick: () => void }) {
  const { data, refetch } = useQuery({
    queryKey: ["assistant", "chats"],
    queryFn: api.assistantChats,
  });
  const current = useAssistant((s) => s.chatId);
  const chats = data ?? [];
  if (chats.length === 0)
    return (
      <div className="flex min-h-0 flex-1 flex-col pt-2">
        <Memory />
        <div className="flex flex-1 flex-col items-center justify-center px-6 text-center">
          <Ori pose="sleeping" size={90} />
          <p className="mt-3 text-[13px] text-ink-3">No conversations yet. They're kept on this computer.</p>
        </div>
      </div>
    );
  return (
    <div className="min-h-0 flex-1 overflow-y-auto pt-2">
      <Memory />
      <ul className="flex flex-col gap-0.5 px-2 pb-2">
        {chats.map((c) => (
          <li key={c.id} className="group relative">
            <button
              className={`w-full rounded-[10px] py-2.5 pl-3.5 pr-11 text-left transition-colors hover:bg-slate-800/80 ${c.id === current ? "bg-slate-800/60" : ""}`}
              onClick={async () => {
                try {
                  await useAssistant.getState().load(c.id);
                  onPick();
                } catch (e) {
                  toastError(e);
                }
              }}
            >
              <p className="truncate text-[13px] text-ink">{c.title}</p>
              <p className="mt-0.5 text-[11.5px] text-ink-3">{ago(c.updated)}</p>
            </button>
            <button
              className="ai-icon absolute right-2 top-1/2 -translate-y-1/2 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
              aria-label={`Delete “${c.title}”`}
              title="Delete"
              onClick={async () => {
                await api.assistantChatDelete(c.id).catch(toastError);
                if (c.id === current) useAssistant.getState().reset();
                void refetch();
              }}
            >
              <Trash2 size={14} />
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
