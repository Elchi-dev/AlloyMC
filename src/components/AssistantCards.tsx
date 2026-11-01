// The richer pieces of the assistant's conversation: project cards, the pack plan (inline
// and full-size for reviewing), and undoing a reply's changes.

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { Check, ChevronRight, ExternalLink, Layers, ListChecks, MessageSquareText, Plus, RotateCcw, Undo2 } from "lucide-react";
import { api, type ProjectCard } from "../lib/api";
import { quickInstall, useAssistant, type PackProposal, type PlanItem, type UndoEntry } from "../lib/assistant";
import { compact } from "../lib/format";
import { openExternal } from "../lib/menus";
import { toastError } from "../lib/store";
import { Dialog } from "./ui";
import { Ori } from "./ori/Ori";

type Ref = { platform: "modrinth" | "curseforge"; projectId: string };

/** Card data for many projects at once, keyed "platform:id". */
function useCards(refs: Ref[]) {
  const key = refs.map((r) => `${r.platform}:${r.projectId}`).join(",");
  return useQuery({
    queryKey: ["cards", key],
    staleTime: 3_600_000,
    enabled: refs.length > 0,
    queryFn: async () => {
      const out = new Map<string, ProjectCard>();
      for (const platform of ["modrinth", "curseforge"] as const) {
        const ids = [...new Set(refs.filter((r) => r.platform === platform).map((r) => r.projectId))];
        if (!ids.length) continue;
        const cards = await api.projectCards(platform, ids).catch(() => [] as ProjectCard[]);
        // Modrinth answers slugs with ids: match those by order of what came back.
        for (const c of cards) out.set(`${platform}:${c.id}`, c);
        for (const id of ids) {
          if (out.has(`${platform}:${id}`)) continue;
          const bySlug = cards.find((c) => c.url.endsWith(`/${id}`));
          if (bySlug) out.set(`${platform}:${id}`, bySlug);
        }
      }
      return out;
    },
  });
}

function Icon({ src, size, title }: { src?: string | null; size: number; title: string }) {
  return src ? (
    <img
      src={src}
      alt=""
      width={size}
      height={size}
      loading="lazy"
      className="shrink-0 rounded-[8px] bg-slate-800 object-cover"
      style={{ width: size, height: size }}
    />
  ) : (
    <span className="grid shrink-0 place-items-center rounded-[8px] bg-slate-800 text-[12px] font-semibold text-ink-3" style={{ width: size, height: size }}>
      {title.slice(0, 1).toUpperCase()}
    </span>
  );
}

// ---------- project cards ----------

export function ProjectCards({ input }: { input: Record<string, any> }) {
  const items: {
    platform?: string;
    project_id?: string;
    title?: string;
    why?: string;
  }[] = Array.isArray(input.items) ? input.items : [];
  const refs: Ref[] = items.map((x) => ({
    platform: x.platform === "curseforge" ? "curseforge" : "modrinth",
    projectId: String(x.project_id ?? ""),
  }));
  const { data } = useCards(refs);
  return (
    <div className="mt-1 flex flex-col gap-2">
      {items.map((x, i) => (
        <ProjectCardView
          key={i}
          item={x}
          r={refs[i]}
          card={data?.get(`${refs[i].platform}:${refs[i].projectId}`)}
          instanceId={typeof input.instance_id === "string" ? input.instance_id : null}
        />
      ))}
    </div>
  );
}

function ProjectCardView({ item, r, card, instanceId }: { item: { title?: string; why?: string }; r: Ref; card?: ProjectCard; instanceId: string | null }) {
  const [state, setState] = useState<"idle" | "adding" | "added">("idle");
  const title = card?.title ?? item.title ?? r.projectId;
  const add = async () => {
    if (!instanceId) return;
    setState("adding");
    try {
      await quickInstall(r.platform, card?.id ?? r.projectId, instanceId, title);
      setState("added");
    } catch (e) {
      toastError(e);
      setState("idle");
    }
  };
  return (
    <article className="overflow-hidden rounded-[12px] border border-line-soft bg-slate-850/80">
      {card?.image && (
        <div className="relative h-[92px] overflow-hidden bg-slate-800">
          <img src={card.image} alt="" loading="lazy" className="h-full w-full object-cover" />
          <div className="absolute inset-0 bg-gradient-to-t from-slate-850 via-slate-850/10 to-transparent" />
        </div>
      )}
      <div className={`flex gap-3 px-3 pb-3 ${card?.image ? "-mt-7" : "pt-3"}`}>
        <div className="relative">
          <Icon src={card?.iconUrl} size={44} title={title} />
        </div>
        <div className="min-w-0 flex-1 pt-0.5">
          <p className={`truncate text-[13.5px] font-semibold text-ink ${card?.image ? "pt-6" : ""}`}>{title}</p>
          <p className="truncate text-[11.5px] text-ink-3">
            {[
              card?.author ? `by ${card.author}` : null,
              card ? `${compact(card.downloads)} downloads` : null,
              r.platform === "modrinth" ? "Modrinth" : "CurseForge",
            ]
              .filter(Boolean)
              .join(", ")}
          </p>
        </div>
      </div>
      <div className="px-3 pb-3">
        {item.why && <p className="text-[12.5px] leading-snug text-ink-2">{item.why}</p>}
        {card?.description && <p className="mt-1 line-clamp-2 text-[12px] leading-snug text-ink-3">{card.description}</p>}
        <div className="mt-2.5 flex items-center gap-2">
          {instanceId && (
            <button className={`btn h-7 px-2.5 text-[12px] ${state === "added" ? "" : "btn-gold"}`} disabled={state !== "idle"} onClick={add}>
              {state === "adding" ? <span className="spinner" /> : state === "added" ? <Check size={13} className="text-grass" /> : <Plus size={13} />}
              {state === "added" ? "Added" : "Add"}
            </button>
          )}
          {card && (
            <button className="btn btn-quiet h-7 px-2.5 text-[12px]" onClick={() => openExternal(card.url)}>
              <ExternalLink size={12} /> Open page
            </button>
          )}
        </div>
      </div>
    </article>
  );
}

// ---------- the pack plan ----------

const loaderLabel: Record<string, string> = {
  fabric: "Fabric",
  quilt: "Quilt",
  forge: "Forge",
  neoforge: "NeoForge",
};

function Chip({ children }: { children: React.ReactNode }) {
  return <span className="rounded-full border border-line bg-slate-800/80 px-2 py-[2px] text-[11.5px] text-ink-2">{children}</span>;
}

/** The plan as it sits in the conversation, waiting for the player. */
export function PlanCard({ p }: { p: PackProposal }) {
  const all = useMemo(() => p.groups.flatMap((g) => g.items), [p]);
  const [picked, setPicked] = useState<Set<string>>(() => new Set(all.map((x) => x.key)));
  const [note, setNote] = useState("");
  const [review, setReview] = useState(false);
  const answerPlan = useAssistant((s) => s.answerPlan);
  const approve = () => answerPlan({ approve: true, selected: [...picked], note });
  const changes = () => answerPlan({ approve: false, selected: [...picked], note });
  return (
    <motion.section initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} className="ai-plan" aria-label={`Plan for ${p.name}`}>
      <div className="flex items-start gap-3 px-4 pt-4">
        <div className="min-w-0 flex-1">
          <p className="text-[11.5px] font-medium text-gold">Pack plan</p>
          <h4 className="mt-0.5 font-pixel text-[20px] leading-tight text-ink">{p.name}</h4>
          <div className="mt-2 flex flex-wrap gap-1.5">
            <Chip>Minecraft {p.gameVersion}</Chip>
            <Chip>{loaderLabel[p.loader] ?? p.loader}</Chip>
            <Chip>{all.length} mods</Chip>
          </div>
        </div>
        <Ori pose="planning" size={58} />
      </div>
      {p.summary && <p className="px-4 pt-3 text-[13px] leading-relaxed text-ink-2">{p.summary}</p>}
      <ul className="mx-4 mt-3 overflow-hidden rounded-[10px] border border-line-soft bg-slate-950/50">
        {p.groups.map((g) => {
          const on = g.items.filter((x) => picked.has(x.key)).length;
          return (
            <li key={g.name} className="flex items-center gap-2.5 border-b border-line-soft/70 px-3 py-2 last:border-b-0">
              <Layers size={13} className="shrink-0 text-ink-3" />
              <span className="min-w-0 flex-1 truncate text-[12.5px] text-ink">{g.name}</span>
              <span className={`text-[11.5px] tabular-nums ${on < g.items.length ? "text-molten" : "text-ink-3"}`}>
                {on === g.items.length ? g.items.length : `${on} of ${g.items.length}`}
              </span>
            </li>
          );
        })}
      </ul>
      <div className="px-4 pt-3">
        <button
          className="flex w-full items-center justify-center gap-2 rounded-[10px] border border-line bg-slate-800/70 py-2 text-[13px] font-medium text-ink transition-colors hover:border-gold/50 hover:bg-slate-800"
          onClick={() => setReview(true)}
        >
          <ListChecks size={15} className="text-gold" /> Review every mod
        </button>
      </div>
      <PlanNote note={note} setNote={setNote} />
      <div className="mt-3 flex items-center gap-2 border-t border-line-soft/80 px-4 py-3">
        <button className="btn btn-quiet h-8 mr-auto" onClick={changes} disabled={!note.trim()} title={note.trim() ? undefined : "Write what to change first"}>
          <MessageSquareText size={13} /> Ask for changes
        </button>
        <button className="btn btn-gold h-8" disabled={picked.size === 0} onClick={approve}>
          <Check size={13} strokeWidth={2.6} /> Approve {picked.size} mods
        </button>
      </div>
      <PlanDialog open={review} onClose={() => setReview(false)} p={p} picked={picked} setPicked={setPicked} onApprove={() => (setReview(false), approve())} />
    </motion.section>
  );
}

function PlanNote({ note, setNote }: { note: string; setNote: (s: string) => void }) {
  return (
    <div className="px-4 pt-3">
      <textarea
        value={note}
        onChange={(e) => setNote(e.target.value)}
        rows={2}
        maxLength={1000}
        placeholder="Anything to change? More farming, no magic, lighter on a laptop…"
        className="block w-full resize-none rounded-[10px] border border-line bg-slate-950/60 px-3 py-2 text-[12.5px] leading-relaxed text-ink outline-none transition-colors placeholder:text-ink-3 focus:border-gold/50"
        aria-label="Notes for Claude"
      />
    </div>
  );
}

/** Reviewing every mod: groups on the left, the group's mods as tiles on the right. */
function PlanDialog({
  open,
  onClose,
  p,
  picked,
  setPicked,
  onApprove,
}: {
  open: boolean;
  onClose: () => void;
  p: PackProposal;
  picked: Set<string>;
  setPicked: (f: (s: Set<string>) => Set<string>) => void;
  onApprove: () => void;
}) {
  const [group, setGroup] = useState(0);
  const all = useMemo(() => p.groups.flatMap((g) => g.items), [p]);
  const { data: cards } = useCards(open ? all : []);
  const g = p.groups[Math.min(group, p.groups.length - 1)];
  const toggle = (keys: string[], on: boolean) =>
    setPicked((s) => {
      const n = new Set(s);
      for (const k of keys) on ? n.add(k) : n.delete(k);
      return n;
    });
  const groupOn = g.items.filter((x) => picked.has(x.key)).length;
  return (
    <Dialog open={open} onClose={onClose} title={`Plan for ${p.name}`} width={920}>
      <div className="flex h-[min(640px,72vh)]">
        <nav className="flex w-[240px] shrink-0 flex-col border-r border-line-soft" aria-label="Groups">
          <div className="px-4 pb-3 pt-4">
            <p className="font-pixel text-[18px] leading-tight text-ink">{p.name}</p>
            <div className="mt-2 flex flex-wrap gap-1.5">
              <Chip>Minecraft {p.gameVersion}</Chip>
              <Chip>{loaderLabel[p.loader] ?? p.loader}</Chip>
            </div>
            <p className="mt-2.5 text-[12px] leading-relaxed text-ink-3">
              {picked.size} of {all.length} mods ticked
            </p>
          </div>
          <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
            {p.groups.map((x, i) => {
              const on = x.items.filter((it) => picked.has(it.key)).length;
              const active = x === g;
              return (
                <li key={x.name}>
                  <button
                    onClick={() => setGroup(i)}
                    className={`flex w-full items-center gap-2 rounded-[8px] px-2.5 py-2 text-left transition-colors ${active ? "bg-slate-800 text-ink" : "text-ink-2 hover:bg-slate-850 hover:text-ink"}`}
                    aria-current={active ? "true" : undefined}
                  >
                    <span className="min-w-0 flex-1 truncate text-[13px]">{x.name}</span>
                    <span className={`text-[11.5px] tabular-nums ${on < x.items.length ? "text-molten" : "text-ink-3"}`}>
                      {on === x.items.length ? x.items.length : `${on}/${x.items.length}`}
                    </span>
                    <ChevronRight size={13} className={`shrink-0 transition-opacity ${active ? "opacity-100" : "opacity-0"}`} />
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center gap-3 border-b border-line-soft px-5 py-3">
            <h4 className="min-w-0 flex-1 truncate text-[15px] font-semibold text-ink">{g.name}</h4>
            <span className="text-[12px] text-ink-3">
              {groupOn} of {g.items.length}
            </span>
            <button
              className="btn btn-quiet h-7 px-2.5 text-[12px]"
              onClick={() =>
                toggle(
                  g.items.map((x) => x.key),
                  groupOn < g.items.length,
                )
              }
            >
              {groupOn < g.items.length ? "Tick all" : "Untick all"}
            </button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto p-4">
            <ul className="grid grid-cols-2 gap-2.5">
              {g.items.map((it) => (
                <PlanTile key={it.key} it={it} card={cards?.get(it.key)} on={picked.has(it.key)} toggle={() => toggle([it.key], !picked.has(it.key))} />
              ))}
            </ul>
          </div>
          <div className="flex items-center justify-end gap-2 border-t border-line-soft px-5 py-3">
            <button className="btn btn-quiet" onClick={onClose}>
              Back
            </button>
            <button className="btn btn-gold" disabled={picked.size === 0} onClick={onApprove}>
              <Check size={14} strokeWidth={2.6} /> Approve {picked.size} mods
            </button>
          </div>
        </div>
      </div>
    </Dialog>
  );
}

function PlanTile({ it, card, on, toggle }: { it: PlanItem; card?: ProjectCard; on: boolean; toggle: () => void }) {
  return (
    <li>
      <label
        className={`group flex h-full cursor-pointer gap-3 rounded-[10px] border p-3 transition-colors ${on ? "border-line bg-slate-850 hover:border-gold/40" : "border-line-soft bg-slate-900/60 opacity-60 hover:opacity-90"}`}
      >
        <Icon src={card?.iconUrl} size={40} title={it.title} />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[13px] font-medium text-ink">{card?.title ?? it.title}</span>
          <span className="mt-0.5 line-clamp-2 block text-[12px] leading-snug text-ink-3">{it.why ?? card?.description ?? ""}</span>
          {card && <span className="mt-1 block text-[11px] text-ink-3/80">{compact(card.downloads)} downloads</span>}
        </span>
        <input type="checkbox" className="ai-check mt-0.5" checked={on} onChange={toggle} aria-label={`Keep ${it.title}`} />
      </label>
    </li>
  );
}

// ---------- undo ----------

export function UndoBar({ k, entry }: { k: string; entry: UndoEntry }) {
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const undoTurn = useAssistant((s) => s.undoTurn);
  if (entry.undone)
    return (
      <p className="flex items-center gap-1.5 pl-1 text-[12px] text-ink-3">
        <RotateCcw size={12} /> Undone: everything from this reply is back as it was.
      </p>
    );
  return (
    <AnimatePresence mode="wait" initial={false}>
      {confirm ? (
        <motion.div
          key="ask"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="flex flex-wrap items-center gap-2 rounded-[10px] border border-line-soft bg-slate-850/80 px-3 py-2"
        >
          <p className="min-w-0 flex-1 text-[12.5px] text-ink-2">Put back how things were before this reply? Later changes to the same instance go too.</p>
          <button className="btn btn-quiet h-7 px-2.5 text-[12px]" onClick={() => setConfirm(false)}>
            Keep
          </button>
          <button
            className="btn h-7 px-2.5 text-[12px]"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await undoTurn(k);
              } catch (e) {
                toastError(e);
              } finally {
                setBusy(false);
                setConfirm(false);
              }
            }}
          >
            {busy ? <span className="spinner" /> : <Undo2 size={13} />} Undo
          </button>
        </motion.div>
      ) : (
        <motion.button
          key="offer"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="flex items-center gap-1.5 self-start rounded-full px-2 py-1 text-[12px] text-ink-3 transition-colors hover:bg-slate-850 hover:text-ink"
          onClick={() => setConfirm(true)}
        >
          <Undo2 size={13} /> Undo these changes
        </motion.button>
      )}
    </AnimatePresence>
  );
}
