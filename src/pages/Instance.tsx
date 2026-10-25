import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowDownToLine, ChevronLeft, Copy, FolderOpen, MoreHorizontal, Plus, Search, Trash2, Wrench } from "lucide-react";
import { InstanceIcon } from "../components/Art";
import { Ori } from "../components/ori/Ori";
import { PlayButton } from "../components/PlayButton";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Empty, Segmented, Switch, Tabs } from "../components/ui";
import { api, newJobId } from "../lib/api";
import { ago, bytes, loaderName, playTime } from "../lib/format";
import { useContent, useInstance, useSettings } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { ContentKind, Instance, InstalledContent } from "../lib/types";

export function InstancePage({ id, tab = "content" }: { id: string; tab?: "content" | "logs" | "settings" }) {
  const { data: instance, error } = useInstance(id);
  const go = useStore((s) => s.go);
  const back = useStore((s) => s.back);

  if (error) return <div className="p-8"><Empty title="Instance not found" body="It may have been deleted." action={<button className="btn" onClick={() => go({ name: "library" })}>Back to library</button>} /></div>;
  if (!instance) return null;

  const setTab = (t: typeof tab) => go({ name: "instance", id, tab: t });

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-line-soft/60 bg-slate-950/40 px-8 pt-5">
        <button className="mb-4 flex items-center gap-1 text-[13px] text-ink-3 hover:text-ink" onClick={back}>
          <ChevronLeft size={15} /> Back
        </button>
        <div className="flex items-center gap-5">
          <InstanceIcon instance={instance} size={72} shared />
          <div className="min-w-0 flex-1">
            <h2 className="rise truncate font-pixel text-[32px] font-medium leading-tight" style={{ ["--i" as string]: 1 } as React.CSSProperties}>{instance.name}</h2>
            <div className="rise mt-2 flex flex-wrap items-center gap-1.5" style={{ ["--i" as string]: 2 } as React.CSSProperties}>
              <span className="chip">Minecraft {instance.gameVersion}</span>
              <span className="chip">
                {loaderName[instance.loader]}
                {instance.loaderVersion ? ` ${instance.loaderVersion}` : ""}
              </span>
              {instance.linkedPack?.name && <span className="chip">From {instance.linkedPack.name} {instance.linkedPack.versionName}</span>}
              <span className="pl-1 text-[12px] text-ink-3">
                {!instance.lastPlayed
                  ? "Not played yet"
                  : instance.playTimeSecs >= 60
                    ? `${playTime(instance.playTimeSecs)}, last ${ago(instance.lastPlayed).toLowerCase()}`
                    : `Last played ${ago(instance.lastPlayed).toLowerCase()}`}
              </span>
            </div>
          </div>
          <MoreMenu instance={instance} />
          <PlayButton instance={instance} size="md" />
        </div>
        <div className="mt-5">
          <Tabs
            value={tab}
            onChange={setTab}
            options={[
              { value: "content", label: "Content" },
              { value: "logs", label: "Logs" },
              { value: "settings", label: "Settings" },
            ]}
          />
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {tab === "content" && <ContentTab instance={instance} />}
        {tab === "logs" && <LogsTab instance={instance} />}
        {tab === "settings" && <SettingsTab instance={instance} />}
      </div>
    </div>
  );
}

function MoreMenu({ instance }: { instance: Instance }) {
  const [open, setOpen] = useState(false);
  const go = useStore((s) => s.go);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  const run = (fn: () => Promise<unknown>) => () => {
    setOpen(false);
    fn().catch(toastError);
  };
  const items = [
    { label: "Open folder", icon: FolderOpen, run: run(() => api.openFolder(instance.id)) },
    { label: "Duplicate", icon: Copy, run: run(() => api.duplicateInstance(instance.id, `${instance.name} copy`)) },
    {
      label: "Repair files",
      icon: Wrench,
      run: run(async () => {
        await api.installInstance(instance.id, newJobId(), true);
        useStore.getState().toast({ kind: "success", text: "Files checked and repaired" });
      }),
    },
    {
      label: "Move to trash",
      icon: Trash2,
      danger: true,
      run: run(async () => {
        await api.deleteInstance(instance.id);
        useStore.getState().toast({ kind: "info", text: `${instance.name} moved to the launcher's trash folder` });
        go({ name: "library" });
      }),
    },
  ];
  return (
    <div ref={ref} className="relative">
      <button className="btn h-11 w-11 justify-center px-0" aria-label="More actions" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <MoreHorizontal size={18} />
      </button>
      {open && (
        <div className="fade-in absolute right-0 top-12 z-30 w-48 origin-top-right rounded-[10px] border border-line bg-slate-850 p-1 shadow-2xl shadow-black/50" style={{ animationDuration: "160ms" }}>
          {items.map((i) => (
            <button key={i.label} onClick={i.run} className={`group/item flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-2 text-left text-[13px] transition-colors hover:bg-slate-800 ${i.danger ? "text-redstone" : ""}`}>
              <i.icon size={15} /> {i.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// ---------- Content ----------

const kindTabs: { value: ContentKind; label: string }[] = [
  { value: "mod", label: "Mods" },
  { value: "resourcepack", label: "Resource packs" },
  { value: "shader", label: "Shaders" },
];

function ContentTab({ instance }: { instance: Instance }) {
  const { data } = useContent(instance.id);
  const go = useStore((s) => s.go);
  const [kind, setKind] = useState<ContentKind>(instance.loader === "vanilla" ? "resourcepack" : "mod");
  const [q, setQ] = useState("");
  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    data?.forEach((d) => (c[d.kind] = (c[d.kind] ?? 0) + 1));
    return c;
  }, [data]);
  const rows = (data ?? []).filter(
    (c) => c.kind === kind && (!q || (c.meta?.title ?? c.fileName).toLowerCase().includes(q.toLowerCase())),
  );
  const discoverKind = kind === "resourcepack" ? "resourcepack" : kind === "shader" ? "shader" : "mod";

  return (
    <div className="flex flex-col gap-4 px-8 py-5">
      <div className="flex items-center gap-3">
        <Segmented
          label="Content type"
          value={kind}
          onChange={setKind}
          options={kindTabs.map((k) => ({ value: k.value, label: <>{k.label} <span className="ml-1 text-ink-3">{counts[k.value] ?? 0}</span></> }))}
        />
        <div className="relative ml-auto w-60">
          <Search size={15} className="absolute left-2.5 top-2.5 text-ink-3" />
          <input className="field pl-8" placeholder="Filter" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <button className="btn btn-gold" onClick={() => go({ name: "discover", target: instance.id, kind: discoverKind })}>
          <Plus size={15} /> Add {kindTabs.find((k) => k.value === kind)!.label.toLowerCase()}
        </button>
      </div>

      {rows.length === 0 ? (
        <Empty
          pose={q ? "searching" : "idle"}
          title={q ? "No matches" : `No ${kindTabs.find((k) => k.value === kind)!.label.toLowerCase()} yet`}
          body={
            kind === "mod" && instance.loader === "vanilla"
              ? "Vanilla instances can't load mods. Create a Fabric, Quilt or NeoForge instance for mods."
              : "Find them in Discover; they're filtered to this instance's version and loader automatically."
          }
        />
      ) : (
        <div className="overflow-hidden rounded-[10px] border border-line-soft">
          {rows.map((c, n) => (
            <ContentRow key={c.path} instance={instance} item={c} index={n} />
          ))}
        </div>
      )}
    </div>
  );
}

function ContentRow({ instance, item, index }: { instance: Instance; item: InstalledContent; index: number }) {
  const title = item.meta?.title ?? item.fileName.replace(/\.(jar|zip)$/, "");
  const source = item.meta?.source ?? "local";
  const toggle = () => api.setContentEnabled(instance.id, item.path, !item.enabled).catch(toastError);
  return (
    <div
      className={`rise group flex items-center gap-3 border-b border-line-soft bg-slate-900/70 px-4 py-2.5 transition-[background,opacity] duration-200 last:border-b-0 hover:bg-slate-850 ${item.enabled ? "" : "opacity-55"}`}
      style={{ ["--i" as string]: index } as React.CSSProperties}
    >
      {item.meta?.iconUrl ? (
        <img src={item.meta.iconUrl} className="h-9 w-9 rounded-[6px] transition-transform duration-200 group-hover:scale-110" alt="" loading="lazy" />
      ) : (
        <div className="grid h-9 w-9 place-items-center rounded-[6px] bg-slate-800 text-[13px] font-semibold text-ink-3">{title.slice(0, 1).toUpperCase()}</div>
      )}
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{title}</div>
        <div className="truncate text-[12px] text-ink-3">{item.meta?.versionName ?? item.fileName}</div>
      </div>
      <span className={`chip ${source === "modrinth" ? "text-grass" : source === "curseforge" ? "text-[#f16436]" : ""}`}>
        {source === "modrinth" ? "Modrinth" : source === "curseforge" ? "CurseForge" : "Local file"}
      </span>
      <span className="w-16 text-right text-[12px] text-ink-3 tabular-nums">{bytes(item.size)}</span>
      <Switch on={item.enabled} onChange={toggle} label={`${item.enabled ? "Disable" : "Enable"} ${title}`} />
      <button
        aria-label={`Remove ${title}`}
        onClick={() => api.removeContent(instance.id, item.path).catch(toastError)}
        className="grid h-8 w-8 place-items-center rounded-[6px] text-ink-3 opacity-0 hover:bg-slate-800 hover:text-redstone group-hover:opacity-100 focus:opacity-100"
      >
        <Trash2 size={15} />
      </button>
    </div>
  );
}

// ---------- Logs ----------

const levelColor = { info: "text-ink-2", warn: "text-gold", error: "text-redstone", debug: "text-ink-3" };

function LogsTab({ instance }: { instance: Instance }) {
  const logs = useStore((s) => s.logs[instance.id]) ?? [];
  const [q, setQ] = useState("");
  const [level, setLevel] = useState<"all" | "warn" | "error">("all");
  const [follow, setFollow] = useState(true);
  const shown = logs.filter(
    (l) =>
      (level === "all" || (level === "warn" ? l.level !== "info" && l.level !== "debug" : l.level === "error")) &&
      (!q || l.line.toLowerCase().includes(q.toLowerCase())),
  );
  return (
    <div className="flex h-full flex-col gap-3 px-8 py-5">
      <div className="flex items-center gap-3">
        <Segmented
          label="Level"
          value={level}
          onChange={setLevel}
          options={[
            { value: "all", label: "Everything" },
            { value: "warn", label: "Warnings" },
            { value: "error", label: "Errors" },
          ]}
        />
        <div className="relative w-64">
          <Search size={15} className="absolute left-2.5 top-2.5 text-ink-3" />
          <input className="field pl-8" placeholder="Search the log" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <label className="ml-auto flex items-center gap-2 text-[13px] text-ink-2">
          <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} className="accent-[var(--color-gold)]" />
          Follow output
        </label>
        <button
          className="btn"
          onClick={() => navigator.clipboard.writeText(logs.map((l) => l.line).join("\n"))}
          disabled={!logs.length}
        >
          <ArrowDownToLine size={15} /> Copy log
        </button>
      </div>
      <LogView lines={shown} empty={logs.length ? "No lines match." : "Start the game to see its output here."} follow={follow} setFollow={setFollow} />
    </div>
  );
}

/** Only the visible lines are in the DOM, so a 5,000-line log scrolls like a short one. */
function LogView({ lines, empty, follow, setFollow }: { lines: { line: string; level: keyof typeof levelColor }[]; empty: string; follow: boolean; setFollow: (v: boolean) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const v = useVirtualizer({ count: lines.length, getScrollElement: () => box.current, estimateSize: () => 19, overscan: 30 });
  useEffect(() => {
    if (follow && lines.length) v.scrollToIndex(lines.length - 1, { align: "end" });
  }, [lines.length, follow, v]);
  return (
    <div
      ref={box}
      data-selectable
      onScroll={(e) => {
        const el = e.currentTarget;
        setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 40);
      }}
      className="min-h-0 flex-1 overflow-auto rounded-[10px] border border-line-soft bg-slate-950/90 p-4 font-[ui-monospace,SFMono-Regular,Menlo,Consolas,monospace] text-[12px] leading-[1.6]"
    >
      {lines.length === 0 ? (
        <div className="flex items-center gap-4 font-sans text-ink-3">
          <Ori pose={empty.startsWith("Start") ? "sleeping" : "searching"} size={64} />
          {empty}
        </div>
      ) : (
        <div style={{ height: v.getTotalSize(), position: "relative" }}>
          {v.getVirtualItems().map((row) => {
            const l = lines[row.index];
            return (
              <div
                key={row.key}
                data-index={row.index}
                ref={v.measureElement}
                className={`log-line absolute left-0 right-0 whitespace-pre-wrap break-words ${levelColor[l.level]}`}
                style={{ transform: `translateY(${row.start}px)` }}
              >
                {l.line}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ---------- Settings ----------

function SettingsTab({ instance }: { instance: Instance }) {
  const { data: settings } = useSettings();
  const [draft, setDraft] = useState(instance);
  useEffect(() => setDraft(instance), [instance]);
  const dirty = JSON.stringify(draft) !== JSON.stringify(instance);
  const memory = draft.memoryMb ?? settings?.defaultMemoryMb ?? 4096;

  const save = async () => {
    try {
      await api.updateInstance(draft);
      useStore.getState().toast({ kind: "success", text: "Instance settings saved" });
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <div className="flex max-w-2xl flex-col gap-7 px-8 py-6">
      <Row label="Name">
        <input className="field" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
      </Row>
      <Row label="Memory" hint={`${(memory / 1024).toFixed(1)} GB. Most modpacks need 6–8 GB; vanilla runs well on 2–4 GB.`}>
        <input
          type="range"
          min={1024}
          max={16384}
          step={512}
          value={memory}
          onChange={(e) => setDraft({ ...draft, memoryMb: Number(e.target.value) })}
          className="w-full accent-[var(--color-gold)]"
        />
      </Row>
      <Row label="Java" hint="Leave empty to use the exact Java version this Minecraft version needs (downloaded automatically).">
        <input
          className="field"
          placeholder="Automatic"
          value={draft.javaPath ?? ""}
          onChange={(e) => setDraft({ ...draft, javaPath: e.target.value || null })}
        />
      </Row>
      <Row label="Java arguments" hint="Added after memory settings, separated by spaces.">
        <input
          className="field font-mono text-[12px]"
          placeholder="-XX:+UseZGC"
          value={draft.jvmArgs.join(" ")}
          onChange={(e) => setDraft({ ...draft, jvmArgs: e.target.value.split(/\s+/).filter(Boolean) })}
        />
      </Row>
      <Row label="Window size" hint="Leave empty for Minecraft's default.">
        <div className="flex items-center gap-2">
          {(["width", "height"] as const).map((k) => (
            <input
              key={k}
              type="number"
              className="field w-28"
              placeholder={k === "width" ? "854" : "480"}
              value={draft.resolution?.[k] ?? ""}
              onChange={(e) => {
                const v = Number(e.target.value);
                const cur = draft.resolution ?? { width: 854, height: 480 };
                setDraft({ ...draft, resolution: v ? { ...cur, [k]: v } : null });
              }}
            />
          ))}
        </div>
      </Row>
      <div className="sticky bottom-0 flex justify-end gap-2 border-t border-line-soft bg-slate-950 py-4">
        <button className="btn btn-quiet" disabled={!dirty} onClick={() => setDraft(instance)}>
          Discard
        </button>
        <button className="btn btn-gold" disabled={!dirty} onClick={save}>
          Save changes
        </button>
      </div>
    </div>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <label className="grid grid-cols-[160px_1fr] items-start gap-4">
      <span className="pt-2 text-[13px] font-medium text-ink-2">{label}</span>
      <span>
        {children}
        {hint && <span className="mt-1.5 block text-[12px] text-ink-3">{hint}</span>}
      </span>
    </label>
  );
}
