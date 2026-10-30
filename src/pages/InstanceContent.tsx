// The Content tab: what's installed, what's wrong, what's out of date, and what needs a
// hand-download from CurseForge, all from one overview call.

import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowUpCircle, Check, ChevronDown, CircleAlert, ExternalLink, Eye, Link2, Plus, Search, Sparkles, Trash2, X } from "lucide-react";
import { Dialog, Empty, Segmented, Switch } from "../components/ui";
import { ProblemList, reportOutcome } from "../components/Problems";
import { api, isTauri } from "../lib/api";
import { bytes } from "../lib/format";
import { useOverview } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { BlockedFile, ContentItem, ContentKind, ContentUpdate, Instance, Overview, Problem } from "../lib/types";

const kindTabs: { value: ContentKind; label: string }[] = [
  { value: "mod", label: "Mods" },
  { value: "resourcepack", label: "Resource packs" },
  { value: "shader", label: "Shaders" },
];

const open = (url: string) => (isTauri ? openUrl(url) : Promise.resolve(window.open(url, "_blank"))).catch(toastError);

export function ContentTab({ instance }: { instance: Instance }) {
  const { data, isLoading } = useOverview(instance.id);
  const go = useStore((s) => s.go);
  const [kind, setKind] = useState<ContentKind>(instance.loader === "vanilla" ? "resourcepack" : "mod");
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [linkOpen, setLinkOpen] = useState(false);
  const anchor = useRef<string | null>(null);

  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    data?.items.forEach((d) => (c[d.kind] = (c[d.kind] ?? 0) + 1));
    return c;
  }, [data]);
  const byPath = useMemo(() => new Map((data?.items ?? []).map((i) => [i.path, i])), [data]);
  const problemsByPath = useMemo(() => {
    const m = new Map<string, Problem[]>();
    data?.report.problems.forEach((p) => m.set(p.path, [...(m.get(p.path) ?? []), p]));
    return m;
  }, [data]);
  const rows = (data?.items ?? []).filter((c) => {
    if (c.kind !== kind) return false;
    if (!q) return true;
    const t = q.toLowerCase();
    return c.title.toLowerCase().includes(t) || c.fileName.toLowerCase().includes(t) || (c.modId ?? "").includes(t);
  });
  useEffect(() => setSelected(new Set()), [kind, instance.id]);

  const select = (path: string, e: React.MouseEvent) => {
    setSelected((cur) => {
      const next = new Set(e.ctrlKey || e.metaKey || e.shiftKey ? cur : []);
      if (e.shiftKey && anchor.current) {
        const a = rows.findIndex((r) => r.path === anchor.current);
        const b = rows.findIndex((r) => r.path === path);
        const [lo, hi] = a < b ? [a, b] : [b, a];
        rows.slice(lo, hi + 1).forEach((r) => next.add(r.path));
      } else if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    if (!e.shiftKey) anchor.current = path;
  };

  const updates = useQuery({
    queryKey: ["contentUpdates", instance.id],
    queryFn: () => api.checkContentUpdates(instance.id),
    enabled: !!data && data.items.length > 0,
    staleTime: 10 * 60_000,
  });

  const errors = data?.report.problems.filter((p) => p.severity === "error") ?? [];
  const warnings = data?.report.problems.filter((p) => p.severity === "warning") ?? [];
  const label = kindTabs.find((k) => k.value === kind)!.label.toLowerCase();
  const discoverKind = kind === "resourcepack" ? "resourcepack" : kind === "shader" ? "shader" : "mod";

  return (
    <div className="flex flex-col gap-4 px-8 py-5">
      <div className="flex flex-wrap items-center gap-3">
        <Segmented
          label="Content type"
          value={kind}
          onChange={setKind}
          options={kindTabs.map((k) => ({ value: k.value, label: <>{k.label} <span className="ml-1 tabular-nums text-ink-3">{counts[k.value] ?? 0}</span></> }))}
        />
        <div className="relative ml-auto w-56">
          <Search size={15} className="absolute left-2.5 top-2.5 text-ink-3" />
          <input className="field pl-8" placeholder="Filter" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <button className="btn" onClick={() => setLinkOpen(true)} title="Paste a Modrinth or CurseForge link">
          <Link2 size={15} /> Link
        </button>
        <button className="btn btn-gold" onClick={() => go({ name: "discover", target: instance.id, kind: discoverKind })}>
          <Plus size={15} /> Add {label}
        </button>
      </div>

      <AnimatePresence initial={false}>
        {errors.length > 0 && (
          <Banner key="errors" tone="error" title={`${errors.length === 1 ? "A problem" : `${errors.length} problems`} would stop this instance from starting`}>
            <ProblemList instanceId={instance.id} problems={errors} />
          </Banner>
        )}
        {warnings.length > 0 && (
          <Banner key="warnings" tone="warning" title={`${warnings.length} ${warnings.length === 1 ? "thing" : "things"} worth a look`} collapsed>
            <ProblemList instanceId={instance.id} problems={warnings} />
          </Banner>
        )}
        {data?.listedMissing && <ListedFiles key="listed" instance={instance} />}
        {!!data?.blocked.length && <BlockedPanel key="blocked" instance={instance} files={data.blocked} />}
        {!!updates.data?.length && <UpdatesPanel key="updates" instance={instance} updates={updates.data} />}
        {!!data?.report.orphans.length && <Orphans key="orphans" instance={instance} overview={data} />}
      </AnimatePresence>

      {isLoading ? (
        <div className="flex flex-col gap-1.5">
          {Array.from({ length: 6 }).map((_, i) => (
            <div key={i} className="skeleton h-[56px] rounded-[8px]" style={{ animationDelay: `${i * 60}ms` }} />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <Empty
          pose={q ? "searching" : "idle"}
          title={q ? "No matches" : `No ${label} yet`}
          body={
            kind === "mod" && instance.loader === "vanilla"
              ? "Vanilla instances can't load mods. Create a Fabric, Quilt, Forge or NeoForge instance for mods."
              : "Find them in Discover (filtered to this instance's version and loader), paste a link, or drop files onto this window."
          }
        />
      ) : (
        <List instance={instance} rows={rows} byPath={byPath} problems={problemsByPath} selected={selected} onSelect={select} />
      )}

      <AnimatePresence>{selected.size > 0 && <SelectionBar instance={instance} selected={selected} byPath={byPath} clear={() => setSelected(new Set())} />}</AnimatePresence>
      <LinkDialog open={linkOpen} onClose={() => setLinkOpen(false)} instance={instance} />
    </div>
  );
}

function Banner({ tone, title, children, collapsed = false }: { tone: "error" | "warning" | "info"; title: string; children: React.ReactNode; collapsed?: boolean }) {
  const [open, setOpen] = useState(!collapsed);
  const color = tone === "error" ? "border-redstone/40 bg-redstone/[0.06]" : tone === "warning" ? "border-gold/30 bg-gold/[0.05]" : "border-line bg-slate-900/70";
  return (
    <motion.section
      layout
      initial={{ opacity: 0, y: -6, scale: 0.99 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: -6, transition: { duration: 0.14 } }}
      transition={{ type: "spring", stiffness: 420, damping: 34 }}
      className={`rounded-[10px] border ${color}`}
    >
      <button className="flex w-full items-center gap-2.5 px-4 py-3 text-left" onClick={() => setOpen((v) => !v)} aria-expanded={open}>
        {tone === "error" ? <CircleAlert size={17} className="attention text-redstone" /> : tone === "warning" ? <Eye size={17} className="text-gold" /> : <Sparkles size={17} className="text-gold" />}
        <span className="text-[14px] font-semibold">{title}</span>
        <ChevronDown size={16} className={`ml-auto text-ink-3 transition-transform duration-200 ${open ? "rotate-180" : ""}`} />
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }} className="overflow-hidden">
            <div className="px-3 pb-3">{children}</div>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.section>
  );
}

function BlockedPanel({ instance, files }: { instance: Instance; files: BlockedFile[] }) {
  return (
    <Banner tone="info" title={`Waiting for ${files.length} ${files.length === 1 ? "file" : "files"} from CurseForge`}>
      <p className="px-1 pb-2.5 text-[13px] text-ink-2">
        Their authors only allow downloads on CurseForge's website. Download each one there; Alloy is watching your Downloads folder and adds them the moment they
        arrive, checked against CurseForge's hash.
      </p>
      <div className="flex flex-col gap-1.5">
        {files.map((f) => (
          <motion.div key={f.fileId} layout initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0, x: 20 }} className="flex items-center gap-3 rounded-[8px] bg-slate-900/80 px-3 py-2">
            {f.iconUrl ? <img src={f.iconUrl} alt="" className="h-8 w-8 rounded-[6px]" /> : <div className="h-8 w-8 rounded-[6px] bg-slate-800" />}
            <div className="min-w-0 flex-1">
              <div className="truncate text-[13px] font-medium">{f.title}</div>
              <div className="truncate text-[12px] text-ink-3">{f.fileName}</div>
            </div>
            <span className="flex items-center gap-1.5 text-[12px] text-ink-3">
              <span className="spinner" /> Watching
            </span>
            <button className="btn h-7 px-2.5 text-[12px] btn-gold" onClick={() => open(f.pageUrl)}>
              <ExternalLink size={13} /> Download page
            </button>
            <button className="grid h-7 w-7 place-items-center rounded-[6px] text-ink-3 hover:bg-slate-800 hover:text-ink" aria-label={`Stop waiting for ${f.title}`} onClick={() => api.dismissBlocked(instance.id, f.fileId).catch(toastError)}>
              <X size={14} />
            </button>
          </motion.div>
        ))}
      </div>
    </Banner>
  );
}

/** The pack ships a list of files it couldn't include; Alloy fetches them instead of the player. */
function ListedFiles({ instance }: { instance: Instance }) {
  const [busy, setBusy] = useState(false);
  const get = async () => {
    setBusy(true);
    try {
      const r = await api.fetchListedFiles(instance.id);
      const t = useStore.getState().toast;
      if (r.installed.length) t({ kind: "success", text: `Added ${r.installed.length} ${r.installed.length === 1 ? "file" : "files"} the pack asked for` });
      if (r.blocked.length) t({ kind: "info", text: `${r.blocked.length} of them can only be downloaded on CurseForge's website; see below.` });
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <motion.div layout initial={{ opacity: 0, y: -6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} className="flex items-center gap-3 rounded-[10px] border border-gold/30 bg-gold/[0.05] px-4 py-3">
      <Sparkles size={17} className="shrink-0 text-gold" />
      <p className="min-w-0 flex-1 text-[13px] text-ink-2">
        <span className="font-semibold text-ink">This modpack lists files it couldn't include.</span> Instead of the pack asking you for each on every start, Alloy can get them now: from CurseForge
        directly where allowed, the rest through your Downloads folder.
      </p>
      <button className="btn btn-gold shrink-0" disabled={busy} onClick={get}>
        {busy ? <span className="spinner" /> : null} Get them
      </button>
    </motion.div>
  );
}

function UpdatesPanel({ instance, updates }: { instance: Instance; updates: ContentUpdate[] }) {
  const qc = useQueryClient();
  const [skip, setSkip] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const chosen = updates.filter((u) => !skip.has(u.path) && u.url);
  const run = async () => {
    setBusy(true);
    try {
      const r = await api.applyContentUpdates(instance.id, chosen);
      const t = useStore.getState().toast;
      if (r.updated.length) t({ kind: "success", text: `Updated ${r.updated.length} ${r.updated.length === 1 ? "file" : "files"}` });
      r.heldBack.forEach(([title, why]) => t({ kind: "info", text: `Kept ${title} as it is: ${why}` }));
      r.failed.forEach(([title, why]) => t({ kind: "error", text: `${title}: ${why}` }));
      await qc.invalidateQueries({ queryKey: ["contentUpdates", instance.id] });
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Banner tone="info" title={`${updates.length} ${updates.length === 1 ? "update" : "updates"} available`} collapsed={updates.length > 6}>
      <div className="flex flex-col gap-1">
        {updates.map((u) => (
          <label key={u.path} className="flex cursor-pointer items-center gap-3 rounded-[8px] px-2 py-1.5 hover:bg-slate-900/80">
            <input
              type="checkbox"
              className="accent-[var(--color-gold)]"
              checked={!skip.has(u.path) && !!u.url}
              disabled={!u.url}
              onChange={() => setSkip((s) => (s.has(u.path) ? (s.delete(u.path), new Set(s)) : new Set(s).add(u.path)))}
            />
            {u.iconUrl ? <img src={u.iconUrl} alt="" className="h-7 w-7 rounded-[5px]" /> : <div className="h-7 w-7 rounded-[5px] bg-slate-800" />}
            <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{u.title}</span>
            <span className="truncate text-[12px] text-ink-3">
              {u.current ?? "?"} <span className="text-ink-2">→</span> <span className="text-ink">{u.versionName}</span>
            </span>
            {u.prerelease && <span className="chip text-[11px]">beta</span>}
            {!u.url && <span className="chip text-[11px]">CurseForge site only</span>}
          </label>
        ))}
      </div>
      <div className="mt-2 flex items-center gap-3 px-1">
        <p className="text-[12px] text-ink-3">Alloy checks the whole set first; an update that would break another mod is kept back.</p>
        <button className="btn btn-gold ml-auto" disabled={busy || !chosen.length} onClick={run}>
          {busy ? <span className="spinner" /> : <ArrowUpCircle size={15} />} Update {chosen.length === updates.length ? "all" : chosen.length}
        </button>
      </div>
    </Banner>
  );
}

function Orphans({ instance, overview }: { instance: Instance; overview: Overview }) {
  const names = overview.report.orphans.map((p) => overview.items.find((i) => i.path === p)?.title ?? p);
  const clean = async () => {
    try {
      const removed = await api.cleanupOrphans(instance.id);
      useStore.getState().toast({ kind: "success", text: `Removed ${removed.join(", ")}` });
    } catch (e) {
      toastError(e);
    }
  };
  return (
    <motion.div layout initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="flex items-center gap-3 rounded-[10px] border border-line-soft bg-slate-900/60 px-4 py-2.5 text-[13px]">
      <Sparkles size={15} className="text-gold" />
      <span className="min-w-0 flex-1 truncate text-ink-2">
        {names.length === 1 ? `${names[0]} was installed for another mod that's gone now.` : `${names.length} libraries were installed for mods that are gone now: ${names.join(", ")}.`}
      </span>
      <button className="btn h-7 px-2.5 text-[12px]" onClick={clean}>
        Clean up
      </button>
    </motion.div>
  );
}

function List(props: { instance: Instance; rows: ContentItem[]; byPath: Map<string, ContentItem>; problems: Map<string, Problem[]>; selected: Set<string>; onSelect: (p: string, e: React.MouseEvent) => void }) {
  const { rows } = props;
  const box = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  // Long lists are virtualised; short ones keep their entrance animation.
  const virtual = rows.length > 60;
  const v = useVirtualizer({ count: rows.length, getScrollElement: () => box.current?.closest("main") ?? null, estimateSize: () => 57, overscan: 12, enabled: virtual });
  if (!virtual) {
    return (
      <div ref={box} className="overflow-hidden rounded-[10px] border border-line-soft">
        {rows.map((c, n) => (
          <Row key={c.path} {...props} item={c} index={n} expanded={expanded === c.path} toggle={() => setExpanded((e) => (e === c.path ? null : c.path))} />
        ))}
      </div>
    );
  }
  return (
    <div ref={box} className="overflow-hidden rounded-[10px] border border-line-soft">
      <div style={{ height: v.getTotalSize(), position: "relative" }}>
        {v.getVirtualItems().map((r) => {
          const c = rows[r.index];
          return (
            <div key={c.path} data-index={r.index} ref={v.measureElement} className="absolute inset-x-0" style={{ transform: `translateY(${r.start}px)` }}>
              <Row {...props} item={c} index={0} expanded={expanded === c.path} toggle={() => setExpanded((e) => (e === c.path ? null : c.path))} />
            </div>
          );
        })}
      </div>
    </div>
  );
}

function ItemIcon({ instance, item }: { instance: Instance; item: ContentItem }) {
  const jarIcon = useQuery({
    queryKey: ["contentIcon", instance.id, item.path],
    queryFn: () => api.contentIcon(instance.id, item.path),
    enabled: !item.meta?.iconUrl && item.hasJarIcon,
    staleTime: Infinity,
  });
  const src = item.meta?.iconUrl ?? jarIcon.data ?? undefined;
  return src ? (
    <img src={src} className="h-9 w-9 shrink-0 rounded-[6px] bg-slate-800 object-cover transition-transform duration-200 group-hover:scale-110" alt="" loading="lazy" />
  ) : (
    <div className="grid h-9 w-9 shrink-0 place-items-center rounded-[6px] bg-slate-800 font-pixel text-[14px] text-ink-3">{item.title.slice(0, 1).toUpperCase()}</div>
  );
}

function Row({ instance, item, index, byPath, problems, selected, onSelect, expanded, toggle }: {
  instance: Instance;
  item: ContentItem;
  index: number;
  byPath: Map<string, ContentItem>;
  problems: Map<string, Problem[]>;
  selected: Set<string>;
  onSelect: (p: string, e: React.MouseEvent) => void;
  expanded: boolean;
  toggle: () => void;
}) {
  const [confirm, setConfirm] = useState(false);
  const source = item.meta?.source ?? "local";
  const mine = problems.get(item.path) ?? [];
  const neededBy = item.requiredBy.map((p) => byPath.get(p)?.title ?? p);
  const toggleEnabled = () => api.setContentEnabled(instance.id, [item.path], !item.enabled).catch(toastError);
  const remove = () => (neededBy.length && !confirm ? setConfirm(true) : api.removeContent(instance.id, [item.path]).catch(toastError));
  const pageUrl =
    source === "modrinth" && item.meta?.projectId
      ? `https://modrinth.com/project/${item.meta.projectId}`
      : source === "curseforge" && item.meta?.projectId
        ? `https://www.curseforge.com/projects/${item.meta.projectId}`
        : null;
  return (
    <div
      className={`rise group border-b border-line-soft bg-slate-900/70 transition-[background,opacity] duration-200 last:border-b-0 hover:bg-slate-850 ${item.enabled ? "" : "opacity-55"} ${selected.has(item.path) ? "row-selected" : ""}`}
      style={{ ["--i" as string]: Math.min(index, 14) } as React.CSSProperties}
    >
      <div
        className="flex cursor-default items-center gap-3 px-4 py-2.5"
        onClick={(e) => (e.ctrlKey || e.metaKey || e.shiftKey ? onSelect(item.path, e) : toggle())}
        onContextMenu={(e) => (e.preventDefault(), onSelect(item.path, { ...e, ctrlKey: true } as React.MouseEvent))}
      >
        <ItemIcon instance={instance} item={item} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate font-medium">{item.title}</span>
            {mine.some((p) => p.severity === "error") && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-redstone" aria-label="Has a problem" />}
          </div>
          <div className="truncate text-[12px] text-ink-3">{item.meta?.versionName ?? item.version ?? item.fileName}</div>
        </div>
        {item.meta?.auto && (
          <span className="chip text-[11px]" title={neededBy.length ? `Needed by ${neededBy.join(", ")}` : "Installed for another mod"}>
            Library
          </span>
        )}
        {neededBy.length > 0 && (
          <span className="hidden text-[12px] text-ink-3 lg:inline" title={neededBy.join(", ")}>
            Needed by {neededBy.length}
          </span>
        )}
        <span className={`chip text-[11px] ${source === "modrinth" ? "text-grass" : source === "curseforge" ? "text-[#f16436]" : ""}`}>
          {source === "modrinth" ? "Modrinth" : source === "curseforge" ? "CurseForge" : "Local file"}
        </span>
        <span className="w-16 text-right text-[12px] tabular-nums text-ink-3">{bytes(item.size)}</span>
        <span onClick={(e) => e.stopPropagation()}>
          <Switch on={item.enabled} onChange={toggleEnabled} label={`${item.enabled ? "Disable" : "Enable"} ${item.title}`} />
        </span>
        <button
          aria-label={`Remove ${item.title}`}
          onClick={(e) => (e.stopPropagation(), remove())}
          className="grid h-8 w-8 place-items-center rounded-[6px] text-ink-3 opacity-0 transition-[opacity,color,background] hover:bg-slate-800 hover:text-redstone focus:opacity-100 group-hover:opacity-100"
        >
          <Trash2 size={15} />
        </button>
      </div>
      <AnimatePresence initial={false}>
        {(expanded || confirm) && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }} className="overflow-hidden">
            <div className="grid grid-cols-[1fr_auto] gap-4 px-4 pb-3.5 pl-16 text-[12px]">
              <div className="flex flex-col gap-1.5 text-ink-2">
                {confirm && (
                  <div className="flex items-center gap-2 rounded-[8px] border border-redstone/40 bg-redstone/[0.06] px-3 py-2 text-[13px] text-ink">
                    <CircleAlert size={15} className="text-redstone" /> {neededBy.join(", ")} {neededBy.length === 1 ? "needs" : "need"} this. Remove it anyway?
                    <button className="btn ml-auto h-7 px-2.5 text-[12px]" onClick={() => setConfirm(false)}>
                      Keep
                    </button>
                    <button className="btn h-7 px-2.5 text-[12px] hover:!border-redstone hover:!text-redstone" onClick={() => api.removeContent(instance.id, [item.path]).catch(toastError)}>
                      Remove
                    </button>
                  </div>
                )}
                {item.description && <p className="max-w-2xl text-[13px] leading-snug">{item.description}</p>}
                <p className="text-ink-3" data-selectable>
                  {item.fileName}
                  {item.modId ? ` · id ${item.modId}` : ""}
                  {item.version ? ` · ${item.version}` : ""}
                  {item.formats.length ? ` · for ${[...new Set(item.formats.map((f) => (f === "legacyforge" ? "Forge" : f[0].toUpperCase() + f.slice(1).replace("forge", "Forge"))))].join(", ")}` : ""}
                </p>
                {item.requires.length > 0 && <p>Needs {item.requires.map((p) => byPath.get(p)?.title ?? p).join(", ")}</p>}
                {neededBy.length > 0 && <p>Needed by {neededBy.join(", ")}</p>}
                {mine.map((p) => (
                  <p key={p.message} className={p.severity === "error" ? "text-redstone" : "text-gold"}>
                    {p.message}
                  </p>
                ))}
              </div>
              {pageUrl && (
                <button className="btn h-7 self-start px-2.5 text-[12px]" onClick={() => open(pageUrl)}>
                  <ExternalLink size={13} /> {source === "modrinth" ? "Modrinth" : "CurseForge"}
                </button>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function SelectionBar({ instance, selected, byPath, clear }: { instance: Instance; selected: Set<string>; byPath: Map<string, ContentItem>; clear: () => void }) {
  const paths = [...selected];
  const outsideNeeds = paths.flatMap((p) => byPath.get(p)?.requiredBy ?? []).filter((p) => !selected.has(p));
  const [confirm, setConfirm] = useState(false);
  const act = (fn: () => Promise<unknown>) => () => fn().then(clear).catch(toastError);
  return (
    <motion.div
      initial={{ y: 30, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      exit={{ y: 30, opacity: 0 }}
      transition={{ type: "spring", stiffness: 500, damping: 34 }}
      className="sticky bottom-4 z-20 mx-auto flex items-center gap-2 rounded-[12px] border border-line bg-slate-850/95 px-3 py-2 shadow-2xl shadow-black/50 backdrop-blur"
    >
      <span className="px-2 text-[13px] font-medium tabular-nums">{paths.length} selected</span>
      <button className="btn h-8" onClick={act(() => api.setContentEnabled(instance.id, paths, true))}>
        Enable
      </button>
      <button className="btn h-8" onClick={act(() => api.setContentEnabled(instance.id, paths, false))}>
        Disable
      </button>
      {confirm ? (
        <button className="btn h-8 !border-redstone !text-redstone" onClick={act(() => api.removeContent(instance.id, paths))}>
          {outsideNeeds.length ? `Others need ${outsideNeeds.length === 1 ? "one" : "some"} of these. Remove anyway` : "Confirm remove"}
        </button>
      ) : (
        <button className="btn h-8 hover:!text-redstone" onClick={() => setConfirm(true)}>
          <Trash2 size={14} /> Remove
        </button>
      )}
      <button className="grid h-8 w-8 place-items-center rounded-[6px] text-ink-3 hover:bg-slate-800 hover:text-ink" aria-label="Clear selection" onClick={clear}>
        <X size={15} />
      </button>
    </motion.div>
  );
}

/** Paste a Modrinth or CurseForge link (or a modpack URL) and install it here. */
export function LinkDialog({ open: isOpen, onClose, instance, initialUrl = "" }: { open: boolean; onClose: () => void; instance?: Instance; initialUrl?: string }) {
  const [url, setUrl] = useState(initialUrl);
  useEffect(() => {
    if (isOpen && initialUrl) setUrl(initialUrl);
  }, [isOpen, initialUrl]);
  const [state, setState] = useState<"idle" | "busy" | "done">("idle");
  const valid = /^(https?:\/\/)?[\w.-]+\.\w+\//.test(url.trim());
  const preview = useQuery({ queryKey: ["link", url.trim()], queryFn: () => api.resolveLink(url.trim()), enabled: isOpen && valid, retry: false });
  useEffect(() => {
    if (!isOpen) setTimeout(() => (setUrl(""), setState("idle")), 200);
  }, [isOpen]);
  const go = useStore((s) => s.go);

  const install = async () => {
    const p = preview.data;
    if (!p) return;
    setState("busy");
    try {
      if (p.platform === "url") {
        onClose();
        await api.importModpack(p.url, crypto.randomUUID());
      } else if (p.platform === "modrinth") {
        const versions = await api.modrinthVersions(p.projectId, p.kind === "modpack" ? undefined : instance?.id);
        const v = (p.versionId && versions.find((x) => x.id === p.versionId)) || versions.find((x) => x.version_type === "release") || versions[0];
        if (!v) throw `${p.title} has no version for ${instance?.gameVersion ?? "this instance"}`;
        if (p.kind === "modpack") {
          onClose();
          const r = await api.installModrinthModpack(v.id, crypto.randomUUID(), p.title);
          useStore.getState().toast({ kind: "success", text: `${p.title} is ready`, action: { label: "Open", run: () => go({ name: "instance", id: r.instance.id }) } });
        } else if (instance) reportOutcome(await api.installModrinthVersion(instance.id, v.id, p.title));
      } else {
        const files = await api.curseforgeFiles(Number(p.projectId), p.kind === "modpack" ? undefined : instance?.id);
        const f = (p.versionId && files.find((x) => String(x.id) === p.versionId)) || files.find((x) => x.releaseType === 1) || files[0];
        if (!f) throw `${p.title} has no file for ${instance?.gameVersion ?? "this instance"}`;
        if (p.kind === "modpack") {
          onClose();
          const r = await api.installCurseForgeModpack(Number(p.projectId), f.id, crypto.randomUUID(), p.title);
          useStore.getState().toast({ kind: "success", text: `${p.title} is ready`, action: { label: "Open", run: () => go({ name: "instance", id: r.instance.id }) } });
        } else if (instance) reportOutcome(await api.installCurseForgeFile(instance.id, Number(p.projectId), f.id, p.title));
      }
      setState("done");
    } catch (e) {
      toastError(e);
      setState("idle");
    }
  };

  const needsInstance = preview.data && preview.data.kind !== "modpack" && !instance;
  return (
    <Dialog open={isOpen} onClose={onClose} title="Install from a link" width={560}>
      <div className="flex flex-col gap-4 p-5">
        <div className="relative">
          <Link2 size={15} className="absolute left-2.5 top-2.5 text-ink-3" />
          <input autoFocus className="field pl-8" placeholder="https://modrinth.com/mod/… or https://www.curseforge.com/minecraft/…" value={url} onChange={(e) => setUrl(e.target.value)} />
        </div>
        <AnimatePresence mode="wait">
          {preview.isFetching ? (
            <motion.div key="loading" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="skeleton h-16 rounded-[10px]" />
          ) : preview.data ? (
            <motion.div key={preview.data.url} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} className="flex items-center gap-3 rounded-[10px] border border-line-soft bg-slate-900/80 p-3">
              {preview.data.iconUrl ? <img src={preview.data.iconUrl} alt="" className="h-12 w-12 rounded-[8px]" /> : <div className="h-12 w-12 rounded-[8px] bg-slate-800" />}
              <div className="min-w-0 flex-1">
                <div className="truncate font-semibold">{preview.data.title}</div>
                <div className="text-[12px] text-ink-3">
                  {preview.data.platform === "modrinth" ? "Modrinth" : preview.data.platform === "curseforge" ? "CurseForge" : "Direct link"} · {preview.data.kind === "modpack" ? "Modpack, installs as a new instance" : `Into ${instance?.name ?? "an instance"}`}
                </div>
              </div>
            </motion.div>
          ) : preview.error ? (
            <motion.p key="err" initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="text-[13px] text-redstone">
              {String(preview.error)}
            </motion.p>
          ) : null}
        </AnimatePresence>
        <div className="flex justify-end gap-2 border-t border-line-soft pt-4">
          <button className="btn btn-quiet" onClick={onClose}>
            Cancel
          </button>
          <button className="btn btn-gold" disabled={!preview.data || state !== "idle" || !!needsInstance} onClick={install}>
            {state === "busy" ? <span className="spinner" /> : state === "done" ? <Check size={15} /> : null} {state === "done" ? "Installed" : "Install"}
          </button>
        </div>
      </div>
    </Dialog>
  );
}
