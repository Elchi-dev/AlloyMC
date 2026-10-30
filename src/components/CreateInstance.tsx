import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link2 } from "lucide-react";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { api, isTauri, newJobId } from "../lib/api";
import { loaderName } from "../lib/format";
import { useGameVersions } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { Loader } from "../lib/types";
import { BlockTile, oreFor } from "./Art";
import { Dialog, Segmented } from "./ui";

const loaders: Loader[] = ["vanilla", "fabric", "quilt", "neoforge", "forge"];

export function CreateInstanceDialog() {
  const open = useStore((s) => s.createOpen);
  const setOpen = useStore((s) => s.setCreateOpen);
  const go = useStore((s) => s.go);
  const [mode, setMode] = useState<"new" | "import">("new");
  const [name, setName] = useState("");
  const [snapshots, setSnapshots] = useState(false);
  const [version, setVersion] = useState("");
  const [loader, setLoader] = useState<Loader>("vanilla");
  const [loaderVersion, setLoaderVersion] = useState("");
  const [fabricApi, setFabricApi] = useState(true);
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);

  const versions = useGameVersions(snapshots);
  const loaderVersions = useQuery({
    queryKey: ["loaderVersions", loader, version],
    queryFn: () => api.loaderVersions(loader, version),
    enabled: open && loader !== "vanilla" && !!version,
  });

  const fabricLike = loader === "fabric" || loader === "quilt";
  const fabricApiVersion = useQuery({
    queryKey: ["fabricApiVersion", loader, version],
    queryFn: () => api.fabricApiVersion(loader, version),
    enabled: open && fabricLike && !!version,
    staleTime: 10 * 60_000,
  });

  useEffect(() => {
    if (versions.data?.length && !versions.data.some((v) => v.id === version)) setVersion(versions.data[0].id);
  }, [versions.data, version]);
  useEffect(() => setLoaderVersion(""), [loader, version]);

  const recommended = useMemo(
    () => loaderVersions.data?.find((v) => v.recommended)?.id ?? loaderVersions.data?.find((v) => v.stable)?.id ?? loaderVersions.data?.[0]?.id ?? "",
    [loaderVersions.data],
  );
  const placeholder = version ? `${loader === "vanilla" ? "" : loaderName[loader] + " "}${version}` : "My instance";
  const noLoaderBuild = loader !== "vanilla" && loaderVersions.isSuccess && !loaderVersions.data.length;

  const close = () => {
    setOpen(false);
    setTimeout(() => {
      setName("");
      setUrl("");
      setMode("new");
    }, 200);
  };

  const create = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const inst = await api.createInstance({
        name: name.trim() || placeholder,
        gameVersion: version,
        loader,
        loaderVersion: loader === "vanilla" ? null : loaderVersion || recommended || null,
        fabricApi: fabricLike ? fabricApi && !!fabricApiVersion.data : false,
      });
      close();
      go({ name: "instance", id: inst.id });
    } catch (err) {
      toastError(err);
    } finally {
      setBusy(false);
    }
  };

  const importPack = async (source: string) => {
    close();
    const jobId = newJobId();
    useStore.getState().toast({ kind: "info", text: "Importing modpack. It appears in your library once the files are in.", action: { label: "Show", run: () => useStore.getState().setDownloadsOpen(true) } });
    try {
      const r = await api.importModpack(source, jobId);
      useStore.getState().toast({ kind: "success", text: `${r.instance.name} is ready to play`, action: { label: "Open", run: () => go({ name: "instance", id: r.instance.id }) } });
      if (r.blocked.length)
        useStore.getState().toast({
          kind: "info",
          text: `${r.blocked.length} file${r.blocked.length > 1 ? "s" : ""} can only be downloaded on CurseForge's website. Alloy lists them on the Content tab and picks them up from Downloads.`,
          action: { label: "Show", run: () => go({ name: "instance", id: r.instance.id, tab: "content" }) },
        });
    } catch (err) {
      toastError(err);
    }
  };

  const pickFile = async () => {
    const path = await openFile({ filters: [{ name: "Modpack", extensions: ["mrpack", "zip"] }] });
    if (typeof path === "string") importPack(path);
  };

  return (
    <Dialog open={open} onClose={close} title="New instance" width={560}>
      <div className="px-5 pt-4">
        <Segmented
          label="How to create"
          value={mode}
          onChange={setMode}
          options={[
            { value: "new", label: "From scratch" },
            { value: "import", label: "Import a modpack" },
          ]}
        />
      </div>

      {mode === "new" ? (
        <form onSubmit={create} className="flex flex-col gap-5 p-5">
          <div className="flex items-center gap-4">
            <span key={oreFor((name || placeholder).toLowerCase().replace(/[^a-z0-9]+/g, "-"))} className="pop-in">
              <BlockTile name={oreFor((name || placeholder).toLowerCase().replace(/[^a-z0-9]+/g, "-"))} size={56} />
            </span>
            <label className="flex-1">
              <span className="mb-1.5 block text-[12px] font-medium text-ink-2">Name</span>
              <input autoFocus className="field" value={name} onChange={(e) => setName(e.target.value)} placeholder={placeholder} />
            </label>
          </div>

          <div>
            <span className="mb-1.5 block text-[12px] font-medium text-ink-2">Loader</span>
            <div className="grid grid-cols-5 gap-1.5">
              {loaders.map((l) => (
                <button
                  type="button"
                  key={l}
                  onClick={() => setLoader(l)}
                  aria-pressed={loader === l}
                  className={`h-9 rounded-[6px] border text-[13px] font-medium transition-[color,background,border-color,transform] duration-150 active:scale-95 ${
                    loader === l ? "border-gold bg-gold/10 text-ink" : "border-line bg-slate-900 text-ink-2 hover:text-ink"
                  }`}
                >
                  {loaderName[l]}
                </button>
              ))}
            </div>
            {(loader === "forge" || loader === "neoforge") && (
              <p className="fade-in mt-2 text-[12px] text-ink-3">Installed from {loaderName[loader]}'s official installer; the first instance of a version takes about half a minute to patch, later ones are instant.</p>
            )}
            {fabricLike && (
              <p className="fade-in mt-2 text-[12px] text-ink-3">{loaderName[loader]} Loader is one version for every Minecraft release. Fabric API is the part made for each one.</p>
            )}
          </div>

          <div className="grid grid-cols-2 gap-3">
            <label>
              <span className="mb-1.5 flex items-center text-[12px] font-medium text-ink-2">
                Minecraft version
                <span className="ml-auto flex items-center gap-1.5 font-normal text-ink-3">
                  <input type="checkbox" checked={snapshots} onChange={(e) => setSnapshots(e.target.checked)} className="accent-[var(--color-gold)]" />
                  Snapshots
                </span>
              </span>
              <select className="field" value={version} onChange={(e) => setVersion(e.target.value)}>
                {versions.data?.map((v) => (
                  <option key={v.id} value={v.id}>
                    {v.id}
                    {v.type !== "release" ? " (snapshot)" : ""}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span className="mb-1.5 block text-[12px] font-medium text-ink-2">Loader version</span>
              <select
                className="field disabled:opacity-50"
                disabled={loader === "vanilla" || !loaderVersions.data?.length}
                value={loader === "vanilla" || noLoaderBuild || loaderVersions.isError ? "none" : loaderVersion || recommended}
                onChange={(e) => setLoaderVersion(e.target.value)}
              >
                {loader === "vanilla" && <option value="none">Not needed</option>}
                {noLoaderBuild && <option value="none">None for {version}</option>}
                {loaderVersions.isError && <option value="none">Couldn't load versions</option>}
                {loaderVersions.data?.map((v) => (
                  <option key={v.id} value={v.id}>
                    {v.id}
                    {v.id === recommended ? " (recommended)" : v.stable ? "" : " (beta)"}
                  </option>
                ))}
              </select>
            </label>
          </div>

          {fabricLike && !noLoaderBuild && (
            <label className={`fade-in -mt-1 flex items-center gap-3 rounded-[8px] border border-line bg-slate-900 px-3 py-2.5 ${fabricApiVersion.data ? "cursor-pointer" : "opacity-60"}`}>
              <input
                type="checkbox"
                className="accent-[var(--color-gold)]"
                checked={fabricApi && !!fabricApiVersion.data}
                disabled={!fabricApiVersion.data}
                onChange={(e) => setFabricApi(e.target.checked)}
              />
              <span className="min-w-0 flex-1">
                <span className="block text-[13px] font-medium">Add Fabric API</span>
                <span className="block truncate text-[12px] text-ink-3">
                  {fabricApiVersion.isLoading
                    ? "Finding the build for " + version + "…"
                    : fabricApiVersion.data
                      ? `${fabricApiVersion.data} · nearly every Fabric mod needs it`
                      : `No Fabric API for ${version}`}
                </span>
              </span>
            </label>
          )}

          <div className="flex justify-end gap-2 border-t border-line-soft pt-4">
            <button type="button" className="btn btn-quiet" onClick={close}>
              Cancel
            </button>
            <button className="btn btn-gold" disabled={busy || !version || noLoaderBuild || (loader !== "vanilla" && !recommended)}>
              Create instance
            </button>
          </div>
        </form>
      ) : (
        <div className="flex flex-col gap-4 p-5">
          <form
            onSubmit={(e) => {
              e.preventDefault();
              importPack(url.trim());
            }}
          >
            <span className="mb-1.5 block text-[12px] font-medium text-ink-2">From a link</span>
            <div className="flex gap-2">
              <div className="relative flex-1">
                <Link2 size={15} className="absolute left-2.5 top-2.5 text-ink-3" />
                <input className="field pl-8" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…/pack.mrpack or a CurseForge .zip" />
              </div>
              <button className="btn btn-gold" disabled={!/^https?:\/\//.test(url.trim())}>
                Import
              </button>
            </div>
          </form>
          <div className="flex items-center gap-3 text-[12px] text-ink-3">
            <span className="h-px flex-1 bg-line-soft" /> or <span className="h-px flex-1 bg-line-soft" />
          </div>
          <button className="btn h-20 justify-center border-dashed" onClick={pickFile} disabled={!isTauri}>
            Choose a .mrpack or CurseForge .zip file
          </button>
          <p className="text-[12px] text-ink-3">Modpacks from Modrinth and CurseForge can also be installed straight from Discover.</p>
        </div>
      )}
    </Dialog>
  );
}
