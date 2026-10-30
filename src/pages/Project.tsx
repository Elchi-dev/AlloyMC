// A project page for anything on Modrinth or CurseForge: description, gallery, versions with
// changelogs, dependencies, and an "Install into…" picker that knows which instances fit.

import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { BookOpen, Bug, Check, ChevronDown, ChevronLeft, Code2, Download, ExternalLink, Heart, MessageCircle, X } from "lucide-react";
import { InstanceIcon } from "../components/Art";
import { reportOutcome } from "../components/Problems";
import { Empty, Tabs } from "../components/ui";
import { api, isTauri, newJobId } from "../lib/api";
import { ago, compact, loaderName } from "../lib/format";
import { useInstances } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { CfFile, ContentKind, Instance, Loader, ModrinthVersion, Platform } from "../lib/types";

const openExternal = (url: string) => (isTauri ? openUrl(url) : Promise.resolve(window.open(url, "_blank"))).catch(toastError);

/** One shape for both platforms, so the page renders the same either way. */
interface Shown {
  title: string;
  summary: string;
  author?: string;
  icon?: string | null;
  color?: string;
  downloads: number;
  follows: number;
  updated?: string | null;
  categories: string[];
  kind: ContentKind;
  gallery: { url: string; title?: string | null }[];
  links: { label: string; url: string; icon: typeof Code2 }[];
  bodyHtml: string | null;
  loaders: string[];
  gameVersions: string[];
}

/** A version or file, normalised. */
interface Release {
  id: string;
  name: string;
  number: string;
  type: "release" | "beta" | "alpha";
  date: string;
  downloads?: number;
  gameVersions: string[];
  loaders: string[];
  changelog?: string | null;
  installable: boolean;
}

const kindOf = (t: string): ContentKind => (t === "modpack" ? "modpack" : t === "resourcepack" ? "resourcepack" : t === "shader" ? "shader" : "mod");
const cfKind = (classId?: number | null): ContentKind => (classId === 4471 ? "modpack" : classId === 12 ? "resourcepack" : classId === 6552 ? "shader" : "mod");
const CF_LOADERS = ["forge", "fabric", "quilt", "neoforge"];

function sanitize(html: string) {
  return DOMPurify.sanitize(html, { ADD_TAGS: ["iframe"], ADD_ATTR: ["allow", "allowfullscreen", "frameborder", "target"] });
}

export function ProjectPage({ platform, id, target }: { platform: Platform; id: string; target?: string }) {
  const back = useStore((s) => s.back);
  const { data: instances } = useInstances();
  const [tab, setTab] = useState<"about" | "gallery" | "versions" | "deps">("about");
  const [lightbox, setLightbox] = useState<number | null>(null);

  const project = useQuery({
    queryKey: ["project", platform, id],
    queryFn: async (): Promise<Shown> => {
      if (platform === "modrinth") {
        const p = await api.modrinthProject(id);
        return {
          title: p.title,
          summary: p.description,
          icon: p.icon_url,
          color: p.color != null ? `#${p.color.toString(16).padStart(6, "0")}` : undefined,
          downloads: p.downloads,
          follows: p.followers,
          updated: p.updated,
          categories: p.categories,
          kind: kindOf(p.project_type),
          gallery: [...p.gallery].sort((a, b) => Number(b.featured) - Number(a.featured)),
          links: [
            p.source_url && { label: "Source", url: p.source_url, icon: Code2 },
            p.issues_url && { label: "Issues", url: p.issues_url, icon: Bug },
            p.wiki_url && { label: "Wiki", url: p.wiki_url, icon: BookOpen },
            p.discord_url && { label: "Discord", url: p.discord_url, icon: MessageCircle },
            { label: "Modrinth", url: `https://modrinth.com/${p.project_type}/${p.slug}`, icon: ExternalLink },
          ].filter(Boolean) as Shown["links"],
          bodyHtml: sanitize(await marked.parse(p.body ?? "")),
          loaders: p.loaders,
          gameVersions: p.game_versions,
        };
      }
      const [m, html] = await Promise.all([api.curseforgeProject(Number(id)), api.curseforgeDescription(Number(id)).catch(() => "")]);
      return {
        title: m.name,
        summary: m.summary,
        author: m.authors[0]?.name,
        icon: m.logo?.thumbnailUrl ?? m.logo?.url,
        downloads: m.downloadCount,
        follows: m.thumbsUpCount,
        updated: m.dateModified,
        categories: m.categories.map((c) => c.name),
        kind: cfKind(m.classId),
        gallery: m.screenshots.map((s) => ({ url: s.url })),
        links: m.links?.websiteUrl ? [{ label: "CurseForge", url: m.links.websiteUrl, icon: ExternalLink }] : [],
        bodyHtml: sanitize(html),
        loaders: [],
        gameVersions: [],
      };
    },
    staleTime: 5 * 60_000,
  });

  const releases = useQuery({
    queryKey: ["projectVersions", platform, id],
    queryFn: async (): Promise<Release[]> => {
      if (platform === "modrinth") {
        const vs = await api.modrinthVersions(id);
        return vs.map((v: ModrinthVersion) => ({
          id: v.id,
          name: v.name,
          number: v.version_number,
          type: v.version_type as Release["type"],
          date: v.date_published,
          downloads: v.downloads,
          gameVersions: v.game_versions,
          loaders: v.loaders,
          changelog: v.changelog,
          installable: true,
        }));
      }
      const fs = await api.curseforgeFiles(Number(id));
      return fs.map((f: CfFile) => ({
        id: String(f.id),
        name: f.displayName,
        number: f.displayName,
        type: f.releaseType === 1 ? "release" : f.releaseType === 2 ? "beta" : "alpha",
        date: f.fileDate ?? "",
        gameVersions: f.gameVersions.filter((g) => /^\d/.test(g)),
        loaders: f.gameVersions.map((g) => g.toLowerCase()).filter((g) => CF_LOADERS.includes(g)),
        installable: !!f.downloadUrl,
      }));
    },
    staleTime: 5 * 60_000,
  });

  const p = project.data;
  const kind = p?.kind ?? "mod";
  // Which instances a release fits. Loader filters only apply to mods and modpacks.
  const fits = (r: Release, inst: Instance) => {
    const gameOk = !r.gameVersions.length || r.gameVersions.includes(inst.gameVersion);
    if (kind !== "mod") return gameOk;
    const ls = inst.loader === "quilt" ? ["quilt", "fabric"] : [inst.loader as Loader];
    return gameOk && (!r.loaders.length || r.loaders.some((l) => ls.includes(l as Loader)));
  };
  const [targetId, setTargetId] = useState<string | undefined>(target);
  const compatible = useMemo(() => {
    const m = new Map<string, Release | undefined>();
    instances?.forEach((i) => {
      const list = (releases.data ?? []).filter((r) => fits(r, i));
      m.set(i.id, list.find((r) => r.type === "release") ?? list[0]);
    });
    return m;
  }, [instances, releases.data, kind]);
  useEffect(() => {
    if (!targetId && instances?.length && kind !== "modpack") setTargetId(instances.find((i) => compatible.get(i.id))?.id ?? instances[0].id);
  }, [instances, compatible, targetId, kind]);
  const targetInstance = instances?.find((i) => i.id === targetId);
  const best = kind === "modpack" ? (releases.data?.find((r) => r.type === "release") ?? releases.data?.[0]) : targetId ? compatible.get(targetId) : undefined;

  const deps = useQuery({
    queryKey: ["deps", best?.id],
    queryFn: () => api.modrinthDependencies(best!.id),
    enabled: platform === "modrinth" && !!best,
  });

  if (project.error) return <div className="p-8"><Empty pose="oops" title="Couldn't load this project" body={String(project.error)} /></div>;

  return (
    <div className="flex h-full flex-col">
      <div className="relative z-10 border-b border-line-soft/60 bg-slate-950/40 px-8 pb-5 pt-5">
        {p?.color && (
          <div className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden>
            <div className="absolute -left-24 -top-24 h-72 w-72 rounded-full opacity-20 blur-3xl" style={{ background: p.color }} />
          </div>
        )}
        <button className="relative mb-4 flex items-center gap-1 text-[13px] text-ink-3 hover:text-ink" onClick={back}>
          <ChevronLeft size={15} /> Back
        </button>
        <div className="relative flex items-start gap-5">
          {p ? (
            p.icon ? <img src={p.icon} alt="" className="pop-in h-24 w-24 shrink-0 rounded-[16px] bg-slate-800 object-cover shadow-xl" /> : <div className="grid h-24 w-24 shrink-0 place-items-center rounded-[16px] bg-slate-800 font-pixel text-[36px] text-ink-3">{p.title[0]}</div>
          ) : (
            <div className="skeleton h-24 w-24 shrink-0 rounded-[16px]" />
          )}
          <div className="min-w-0 flex-1">
            {p ? (
              <>
                <h2 className="rise truncate font-pixel text-[30px] font-medium leading-tight">{p.title}</h2>
                <p className="rise mt-1 max-w-2xl text-[14px] text-ink-2" style={{ ["--i" as string]: 1 } as React.CSSProperties}>
                  {p.summary}
                </p>
                <div className="rise mt-3 flex flex-wrap items-center gap-3 text-[12px] text-ink-3" style={{ ["--i" as string]: 2 } as React.CSSProperties}>
                  {p.author && <span>by {p.author}</span>}
                  <span className="flex items-center gap-1"><Download size={12} /> {compact(p.downloads)}</span>
                  <span className="flex items-center gap-1"><Heart size={12} /> {compact(p.follows)}</span>
                  {p.updated && <span>Updated {ago(Date.parse(p.updated) / 1000).toLowerCase()}</span>}
                  {p.categories.slice(0, 5).map((c) => <span key={c} className="chip capitalize">{c.replace(/-/g, " ")}</span>)}
                </div>
              </>
            ) : (
              <div className="flex flex-col gap-2"><div className="skeleton h-8 w-72 rounded" /><div className="skeleton h-4 w-[28rem] rounded" /></div>
            )}
          </div>
          <InstallBox platform={platform} id={id} kind={kind} title={p?.title} instances={instances ?? []} targetId={targetId} setTargetId={setTargetId} compatible={compatible} best={best} loading={releases.isLoading} />
        </div>
        <div className="relative mt-5 flex items-center">
          <Tabs
            value={tab}
            onChange={setTab}
            options={[
              { value: "about", label: "About" },
              ...(p?.gallery.length ? [{ value: "gallery" as const, label: `Gallery ${p.gallery.length}` }] : []),
              { value: "versions", label: `Versions${releases.data ? ` ${releases.data.length}` : ""}` },
              ...(platform === "modrinth" ? [{ value: "deps" as const, label: "Dependencies" }] : []),
            ]}
          />
          <div className="ml-auto flex gap-1.5 pb-2">
            {p?.links.map((l) => (
              <button key={l.url} className="btn h-7 px-2.5 text-[12px]" onClick={() => openExternal(l.url)}>
                <l.icon size={13} /> {l.label}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-8 py-6">
        {tab === "about" && (p ? <Body html={p.bodyHtml} /> : <div className="flex flex-col gap-2">{[0, 1, 2, 3].map((i) => <div key={i} className="skeleton h-4 rounded" style={{ width: `${90 - i * 12}%` }} />)}</div>)}
        {tab === "gallery" && p && (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-3">
            {p.gallery.map((g, i) => (
              <motion.button key={g.url} layoutId={`shot-${g.url}`} onClick={() => setLightbox(i)} className="rise overflow-hidden rounded-[10px] bg-slate-800" style={{ ["--i" as string]: Math.min(i, 12) } as React.CSSProperties}>
                <img src={g.url} alt={g.title ?? ""} loading="lazy" className="aspect-video w-full object-cover transition-transform duration-300 hover:scale-105" />
              </motion.button>
            ))}
          </div>
        )}
        {tab === "versions" && <Versions releases={releases.data} loading={releases.isLoading} target={targetInstance} fits={fits} platform={platform} projectId={id} kind={kind} title={p?.title} />}
        {tab === "deps" && <Deps deps={deps.data} loading={deps.isLoading} target={targetInstance} />}
      </div>

      <AnimatePresence>
        {lightbox != null && p && (
          <motion.div className="fixed inset-0 z-50 grid place-items-center bg-black/80 p-10" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onClick={() => setLightbox(null)}>
            <motion.img layoutId={`shot-${p.gallery[lightbox].url}`} src={p.gallery[lightbox].url} alt="" className="max-h-full max-w-full rounded-[12px] shadow-2xl" onClick={(e) => (e.stopPropagation(), setLightbox((l) => ((l ?? 0) + 1) % p.gallery.length))} />
            <button className="absolute right-6 top-6 grid h-10 w-10 place-items-center rounded-full bg-slate-800/80 text-ink hover:rotate-90 hover:bg-slate-700" aria-label="Close" onClick={() => setLightbox(null)}>
              <X size={18} />
            </button>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

/** Project descriptions, sanitised; links open in the browser instead of inside Alloy. */
function Body({ html }: { html: string | null }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onClick = (e: MouseEvent) => {
      const a = (e.target as HTMLElement).closest("a");
      if (a?.href) {
        e.preventDefault();
        openExternal(a.href);
      }
    };
    el.addEventListener("click", onClick);
    return () => el.removeEventListener("click", onClick);
  }, [html]);
  if (!html) return <p className="text-ink-3">No description.</p>;
  return <div ref={ref} className="prose-alloy fade-in max-w-3xl" data-selectable dangerouslySetInnerHTML={{ __html: html }} />;
}

function InstallBox({ platform, id, kind, title, instances, targetId, setTargetId, compatible, best, loading }: {
  platform: Platform;
  id: string;
  kind: ContentKind;
  title?: string;
  instances: Instance[];
  targetId?: string;
  setTargetId: (id: string) => void;
  compatible: Map<string, Release | undefined>;
  best?: Release;
  loading: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [state, setState] = useState<"idle" | "busy" | "done">("idle");
  const go = useStore((s) => s.go);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  useEffect(() => setState("idle"), [targetId]);
  const target = instances.find((i) => i.id === targetId);

  const install = async (release = best) => {
    if (!release) return;
    setState("busy");
    try {
      if (kind === "modpack") {
        useStore.getState().toast({ kind: "info", text: `Installing ${title}. It's in the downloads panel.` });
        const r = platform === "modrinth" ? await api.installModrinthModpack(release.id, newJobId(), title) : await api.installCurseForgeModpack(Number(id), Number(release.id), newJobId(), title);
        useStore.getState().toast({ kind: "success", text: `${title} is ready`, action: { label: "Open", run: () => go({ name: "instance", id: r.instance.id }) } });
      } else if (target) {
        const out = platform === "modrinth" ? await api.installModrinthVersion(target.id, release.id, title) : await api.installCurseForgeFile(target.id, Number(id), Number(release.id), title);
        reportOutcome(out);
      }
      setState("done");
    } catch (e) {
      toastError(e);
      setState("idle");
    }
  };

  return (
    <div ref={ref} className="relative flex shrink-0 flex-col items-end gap-2">
      <div className="flex">
        <button className="btn btn-gold h-11 rounded-r-none px-5 text-[14px]" disabled={loading || !best || state !== "idle" || (kind !== "modpack" && !target)} onClick={() => install()}>
          {state === "busy" ? <span className="spinner" /> : state === "done" ? <Check size={16} className="pop-in" /> : <Download size={16} />}
          {state === "done" ? "Installed" : kind === "modpack" ? "Install modpack" : target ? `Install into ${target.name}` : "Install"}
        </button>
        {kind !== "modpack" && (
          <button className="btn btn-gold h-11 rounded-l-none border-l border-gold-deep/50 px-2" aria-label="Choose instance" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
            <ChevronDown size={16} className={`transition-transform ${open ? "rotate-180" : ""}`} />
          </button>
        )}
      </div>
      <span className="text-[12px] text-ink-3">
        {loading ? "Checking versions…" : best ? `${best.number}${kind !== "modpack" && target ? ` for ${target.gameVersion} ${loaderName[target.loader]}` : ""}` : target ? `No version for ${target.gameVersion} ${loaderName[target.loader]}` : ""}
      </span>
      <AnimatePresence>
        {open && (
          <motion.div initial={{ opacity: 0, y: -4, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -4, scale: 0.98 }} transition={{ duration: 0.14 }} className="absolute right-0 top-12 z-30 max-h-80 w-80 origin-top-right overflow-y-auto rounded-[10px] border border-line bg-slate-850 p-1 shadow-2xl shadow-black/50">
            {instances.map((i) => {
              const r = compatible.get(i.id);
              return (
                <button key={i.id} onClick={() => (setTargetId(i.id), setOpen(false))} className={`flex w-full items-center gap-2.5 rounded-[7px] px-2 py-1.5 text-left transition-colors hover:bg-slate-800 ${r ? "" : "opacity-50"}`}>
                  <InstanceIcon instance={i} size={28} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] font-medium">{i.name}</span>
                    <span className="block truncate text-[11px] text-ink-3">{r ? `${r.number}` : `Nothing for ${i.gameVersion} ${loaderName[i.loader]}`}</span>
                  </span>
                  {i.id === targetId && <Check size={14} className="text-gold" />}
                </button>
              );
            })}
            {!instances.length && <p className="px-3 py-2 text-[13px] text-ink-3">Create an instance first.</p>}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function Versions({ releases, loading, target, fits, platform, projectId, kind, title }: {
  releases?: Release[];
  loading: boolean;
  target?: Instance;
  fits: (r: Release, i: Instance) => boolean;
  platform: Platform;
  projectId: string;
  kind: ContentKind;
  title?: string;
}) {
  const [onlyFit, setOnlyFit] = useState(true);
  const [openId, setOpenId] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  if (loading) return <div className="flex flex-col gap-1.5">{[0, 1, 2, 3, 4].map((i) => <div key={i} className="skeleton h-12 rounded-[8px]" />)}</div>;
  const list = (releases ?? []).filter((r) => !onlyFit || !target || kind === "modpack" || fits(r, target));
  const install = async (r: Release) => {
    setBusy(r.id);
    try {
      if (kind === "modpack") await (platform === "modrinth" ? api.installModrinthModpack(r.id, newJobId(), title) : api.installCurseForgeModpack(Number(projectId), Number(r.id), newJobId(), title));
      else if (target) reportOutcome(await (platform === "modrinth" ? api.installModrinthVersion(target.id, r.id, title) : api.installCurseForgeFile(target.id, Number(projectId), Number(r.id), title)));
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(null);
    }
  };
  return (
    <div className="flex max-w-4xl flex-col gap-3">
      {target && kind !== "modpack" && (
        <label className="flex items-center gap-2 text-[13px] text-ink-2">
          <input type="checkbox" className="accent-[var(--color-gold)]" checked={onlyFit} onChange={(e) => setOnlyFit(e.target.checked)} />
          Only versions for {target.name} ({target.gameVersion} {loaderName[target.loader]})
        </label>
      )}
      <div className="overflow-hidden rounded-[10px] border border-line-soft">
        {list.slice(0, 120).map((r, i) => (
          <div key={r.id} className="rise border-b border-line-soft bg-slate-900/70 last:border-b-0" style={{ ["--i" as string]: Math.min(i, 14) } as React.CSSProperties}>
            <div className="flex items-center gap-3 px-4 py-2.5">
              <span className={`w-14 shrink-0 rounded-[4px] py-0.5 text-center text-[11px] font-semibold ${r.type === "release" ? "bg-grass/15 text-grass" : r.type === "beta" ? "bg-gold/15 text-gold" : "bg-redstone/15 text-redstone"}`}>{r.type}</span>
              <button className="min-w-0 flex-1 text-left" onClick={() => setOpenId((o) => (o === r.id ? null : r.id))}>
                <span className="block truncate text-[13px] font-medium">{r.number}</span>
                <span className="block truncate text-[12px] text-ink-3">
                  {r.gameVersions.slice(0, 6).join(", ")}
                  {r.gameVersions.length > 6 ? ` +${r.gameVersions.length - 6}` : ""}
                  {r.loaders.length ? ` · ${r.loaders.map((l) => loaderName[l as Loader] ?? l).join(", ")}` : ""}
                </span>
              </button>
              <span className="hidden text-[12px] text-ink-3 md:inline">{r.date ? ago(Date.parse(r.date) / 1000) : ""}</span>
              {r.downloads != null && <span className="hidden w-16 text-right text-[12px] tabular-nums text-ink-3 md:inline">{compact(r.downloads)}</span>}
              <button className="btn h-7 px-2.5 text-[12px]" disabled={busy === r.id || !r.installable || (kind !== "modpack" && !target)} onClick={() => install(r)} title={r.installable ? undefined : "Only downloadable on CurseForge's website"}>
                {busy === r.id ? <span className="spinner" /> : <Download size={13} />} Install
              </button>
            </div>
            <AnimatePresence initial={false}>
              {openId === r.id && r.changelog && (
                <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden">
                  <div className="px-4 pb-4 pl-[5.5rem]">
                    <Changelog text={r.changelog} />
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        ))}
        {!list.length && <p className="px-4 py-6 text-center text-[13px] text-ink-3">No versions{target && onlyFit ? ` for ${target.gameVersion} ${loaderName[target.loader]}` : ""}.</p>}
      </div>
    </div>
  );
}

function Changelog({ text }: { text: string }) {
  const html = useMemo(() => sanitize(marked.parse(text, { async: false }) as string), [text]);
  return <Body html={html} />;
}

function Deps({ deps, loading, target }: { deps?: { projectId: string; title: string; iconUrl?: string | null; kind: string; slug: string }[]; loading: boolean; target?: Instance }) {
  const go = useStore((s) => s.go);
  if (loading) return <div className="skeleton h-24 max-w-3xl rounded-[10px]" />;
  if (!deps?.length) return <Empty pose="done" title="No dependencies" body="This version works on its own." />;
  const groups: [string, string][] = [
    ["required", "Required"],
    ["optional", "Optional"],
    ["incompatible", "Doesn't work with"],
    ["embedded", "Bundled inside"],
  ];
  return (
    <div className="flex max-w-3xl flex-col gap-5">
      <p className="text-[13px] text-ink-3">Alloy installs required dependencies with the project{target ? ` and skips what ${target.name} already has` : ""}.</p>
      {groups.map(([k, label]) => {
        const list = deps.filter((d) => d.kind === k);
        if (!list.length) return null;
        return (
          <section key={k}>
            <h3 className="mb-2 text-[12px] font-semibold uppercase tracking-wide text-ink-3">{label}</h3>
            <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-2">
              {list.map((d, i) => (
                <button key={d.projectId} onClick={() => go({ name: "project", platform: "modrinth", id: d.projectId, target: target?.id })} className="card rise flex items-center gap-3 rounded-[10px] p-3 text-left" style={{ ["--i" as string]: i } as React.CSSProperties}>
                  {d.iconUrl ? <img src={d.iconUrl} alt="" className="h-9 w-9 rounded-[6px]" /> : <div className="h-9 w-9 rounded-[6px] bg-slate-800" />}
                  <span className="truncate text-[13px] font-medium">{d.title}</span>
                </button>
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}
