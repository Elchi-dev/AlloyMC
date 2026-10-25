import { useEffect, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Check, Download, Heart, Search } from "lucide-react";
import { Empty, Segmented } from "../components/ui";
import { api, newJobId } from "../lib/api";
import { ago, compact, loaderName } from "../lib/format";
import { useInstances, useSettings } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { ContentHit, ContentKind, Instance, Platform } from "../lib/types";

const kinds: { value: ContentKind; label: string }[] = [
  { value: "modpack", label: "Modpacks" },
  { value: "mod", label: "Mods" },
  { value: "resourcepack", label: "Resource packs" },
  { value: "shader", label: "Shaders" },
];

function useDebounced<T>(v: T, ms = 250) {
  const [d, setD] = useState(v);
  useEffect(() => {
    const t = setTimeout(() => setD(v), ms);
    return () => clearTimeout(t);
  }, [v, ms]);
  return d;
}

export function DiscoverPage({ target, kind: initialKind }: { target?: string; kind?: ContentKind }) {
  const { data: instances } = useInstances();
  const { data: settings } = useSettings();
  const info = useQuery({ queryKey: ["appInfo"], queryFn: api.appInfo, staleTime: Infinity });
  const cfReady = !!info.data?.curseforgeBuiltin || !!settings?.curseforgeApiKey;
  const [platform, setPlatform] = useState<Platform>("modrinth");
  const [kind, setKind] = useState<ContentKind>(initialKind ?? (target ? "mod" : "modpack"));
  const [targetId, setTargetId] = useState<string | undefined>(target);
  const [q, setQ] = useState("");
  const [sort, setSort] = useState("relevance");
  const query = useDebounced(q);
  const instance = instances?.find((i) => i.id === targetId);
  const forPack = kind === "modpack";

  useEffect(() => {
    if (!targetId && !forPack && instances?.length) setTargetId(instances[0].id);
  }, [instances, targetId, forPack]);

  const results = useQuery({
    queryKey: ["search", platform, kind, query, sort, forPack ? null : instance?.gameVersion, forPack ? null : instance?.loader],
    enabled: platform === "modrinth" || cfReady,
    queryFn: () =>
      api.search(platform, {
        query,
        kind,
        sort,
        limit: 30,
        gameVersion: forPack ? undefined : instance?.gameVersion,
        loader: forPack || !instance || instance.loader === "vanilla" ? undefined : instance.loader,
      }),
    placeholderData: keepPreviousData,
  });

  return (
    <div className="mx-auto flex max-w-[1100px] flex-col gap-4 px-8 py-6">
      <div className="rise flex flex-wrap items-center gap-3">
        <div className="relative min-w-[280px] flex-1">
          <Search size={16} className="absolute left-3 top-[11px] text-ink-3" />
          <input
            autoFocus
            className="field h-10 pl-9 text-[14px]"
            placeholder={`Search ${kinds.find((k) => k.value === kind)!.label.toLowerCase()} on ${platform === "modrinth" ? "Modrinth" : "CurseForge"}`}
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </div>
        <Segmented
          label="Platform"
          value={platform}
          onChange={setPlatform}
          options={[
            { value: "modrinth", label: "Modrinth" },
            {
              value: "curseforge",
              label: cfReady ? "CurseForge" : (
                <span className="flex items-center gap-1.5">
                  CurseForge <span className="rounded-[4px] bg-gold/15 px-1 text-[10px] font-semibold text-gold">WIP</span>
                </span>
              ),
            },
          ]}
        />
      </div>
      <div className="rise flex flex-wrap items-center gap-3" style={{ ["--i" as string]: 1 } as React.CSSProperties}>
        <Segmented label="Type" value={kind} onChange={setKind} options={kinds} />
        {!forPack && (
          <label className="flex items-center gap-2 text-[13px] text-ink-2">
            Installing into
            <select className="field h-8 w-56" value={targetId ?? ""} onChange={(e) => setTargetId(e.target.value)}>
              {instances?.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.name} ({i.gameVersion} {loaderName[i.loader]})
                </option>
              ))}
            </select>
          </label>
        )}
        <select className="field ml-auto h-8 w-40" value={sort} onChange={(e) => setSort(e.target.value)} aria-label="Sort by">
          <option value="relevance">Most relevant</option>
          <option value="downloads">Most downloaded</option>
          <option value="follows">Most followed</option>
          <option value="updated">Recently updated</option>
          <option value="newest">Newest</option>
        </select>
      </div>

      {platform === "curseforge" && !cfReady ? (
        <Empty
          pose="working"
          title="CurseForge is a work in progress"
          body="It arrives with the Alloy server, which keeps the API key off your machine. Modrinth works fully today. Have your own CurseForge key? Paste it in Settings to try it now."
          action={<button className="btn" onClick={() => useStore.getState().go({ name: "settings" })}>Open settings</button>}
        />
      ) : results.error ? (
        <Empty pose="oops" title="Search failed" body={String(results.error)} />
      ) : (
        <>
          {results.data && (
            <p className="text-[12px] text-ink-3">
              {compact(results.data.total)} results
              {!forPack && instance ? ` for ${instance.gameVersion}${instance.loader !== "vanilla" ? " " + loaderName[instance.loader] : ""}` : ""}
            </p>
          )}
          <div className={`flex flex-col gap-2 transition-opacity ${results.isFetching ? "opacity-60" : ""}`}>
            {results.data?.hits.map((h, n) => <ResultRow key={`${h.source}-${h.id}`} hit={h} index={n} instance={forPack ? undefined : instance} />)}
          </div>
        </>
      )}
    </div>
  );
}

function ResultRow({ hit, instance, index }: { hit: ContentHit; instance?: Instance; index: number }) {
  const [state, setState] = useState<"idle" | "busy" | "done">("idle");
  const go = useStore((s) => s.go);

  const install = async () => {
    setState("busy");
    try {
      if (hit.source === "curseforge") throw "CurseForge installs from Discover arrive in Phase 2. Import CurseForge modpacks as a .zip for now.";
      const versions = await api.modrinthVersions(hit.id, hit.kind === "modpack" ? undefined : instance?.id);
      const v = versions.find((x) => x.version_type === "release") ?? versions[0];
      if (!v) throw `${hit.title} has no version for ${instance?.gameVersion} ${instance ? loaderName[instance.loader] : ""}`;
      if (hit.kind === "modpack") {
        useStore.getState().toast({ kind: "info", text: `Installing ${hit.title}. You can keep browsing.` });
        const r = await api.installModrinthModpack(v.id, newJobId());
        useStore.getState().toast({
          kind: "success",
          text: `${hit.title} is ready`,
          action: { label: "Open", run: () => go({ name: "instance", id: r.instance.id }) },
        });
      } else if (instance) {
        const names = await api.installModrinthVersion(instance.id, v.id);
        if (names.length > 1) useStore.getState().toast({ kind: "success", text: `Also installed ${names.slice(1).join(", ")}` });
      }
      setState("done");
    } catch (e) {
      toastError(e);
      setState("idle");
    }
  };

  const tint = hit.color != null ? `#${hit.color.toString(16).padStart(6, "0")}` : undefined;
  return (
    <article className="card rise group flex items-center gap-4 rounded-[10px] p-3.5 pr-4" style={{ ["--i" as string]: index } as React.CSSProperties}>
      <div className="relative shrink-0">
        {tint && <div className="absolute inset-0 rounded-[10px] opacity-30 blur-xl transition-opacity duration-300 group-hover:opacity-60" style={{ background: tint }} aria-hidden />}
        {hit.iconUrl ? (
          <img src={hit.iconUrl} alt="" className="relative h-16 w-16 rounded-[10px] bg-slate-800 object-cover transition-transform duration-300 group-hover:scale-105 group-hover:-rotate-2" loading="lazy" />
        ) : (
          <div className="relative grid h-16 w-16 place-items-center rounded-[10px] bg-slate-800 font-pixel text-[24px] text-ink-3">{hit.title.slice(0, 1)}</div>
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <h3 className="truncate text-[15px] font-semibold">{hit.title}</h3>
          <span className="shrink-0 text-[12px] text-ink-3">by {hit.author}</span>
        </div>
        <p className="mt-0.5 line-clamp-2 text-[13px] text-ink-2">{hit.description}</p>
        <div className="mt-2 flex flex-wrap items-center gap-3 text-[12px] text-ink-3">
          <span className="flex items-center gap-1"><Download size={12} /> {compact(hit.downloads)}</span>
          <span className="flex items-center gap-1"><Heart size={12} /> {compact(hit.follows)}</span>
          {hit.updated && <span>Updated {ago(Date.parse(hit.updated) / 1000).toLowerCase()}</span>}
          {hit.categories.slice(0, 3).map((c) => (
            <span key={c} className="capitalize">{c.replace(/-/g, " ")}</span>
          ))}
        </div>
      </div>
      <button
        className="btn shrink-0 hover:border-gold hover:text-gold"
        onClick={install}
        disabled={state !== "idle" || (hit.kind !== "modpack" && !instance)}
      >
        {state === "done" ? (
          <span className="pop-in flex items-center gap-1.5"><Check size={15} className="text-grass" /> Installed</span>
        ) : state === "busy" ? (
          <span className="flex items-center gap-2"><span className="spinner" /> Installing</span>
        ) : (
          <><Download size={15} /> {hit.kind === "modpack" ? "Install pack" : "Install"}</>
        )}
      </button>
    </article>
  );
}
