// Brings instances over from Prism Launcher, MultiMC, the CurseForge App and the Modrinth App.

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { motion } from "motion/react";
import { FolderSearch } from "lucide-react";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { api, isTauri } from "../lib/api";
import { loaderName } from "../lib/format";
import { queryClient } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { FoundInstance, OtherLauncher } from "../lib/types";

const launcherName: Record<OtherLauncher, string> = {
  prism: "Prism Launcher",
  multiMc: "MultiMC",
  curseForgeApp: "CurseForge App",
  modrinthApp: "Modrinth App",
};

export function OtherLaunchers({ onDone }: { onDone: () => void }) {
  const discovered = useQuery({ queryKey: ["importDiscover"], queryFn: api.importDiscover, staleTime: 30_000 });
  const [picked, setPicked] = useState<FoundInstance[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);

  const all = useMemo(() => {
    const seen = new Set<string>();
    return [...picked, ...(discovered.data ?? [])].filter((f) => !seen.has(f.gameDir) && !!seen.add(f.gameDir));
  }, [picked, discovered.data]);
  const groups = useMemo(() => {
    const m = new Map<OtherLauncher, FoundInstance[]>();
    for (const f of all) m.set(f.from, [...(m.get(f.from) ?? []), f]);
    return [...m.entries()];
  }, [all]);

  const toggle = (dir: string) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(dir)) n.delete(dir);
      else n.add(dir);
      return n;
    });
  const chooseFolder = async () => {
    const path = await openFile({ directory: true });
    if (typeof path !== "string") return;
    const found = await api.importInspect(path);
    if (!found.length) {
      useStore.getState().toast({ kind: "error", text: "No Prism, MultiMC, CurseForge App or Modrinth App instance in that folder" });
      return;
    }
    setPicked((p) => [...found, ...p]);
    setSelected((s) => new Set([...s, ...found.map((f) => f.gameDir)]));
  };
  const importAll = async () => {
    const list = all.filter((f) => selected.has(f.gameDir));
    setBusy(true);
    onDone();
    useStore.getState().toast({ kind: "info", text: `Copying ${list.length === 1 ? list[0].name : `${list.length} instances`}. The originals aren't changed.`, action: { label: "Show", run: () => useStore.getState().setDownloadsOpen(true) } });
    let ok = 0;
    for (const f of list) {
      try {
        await api.importInstance(f);
        ok++;
      } catch (e) {
        toastError(e);
      }
    }
    setBusy(false);
    queryClient.invalidateQueries({ queryKey: ["instances"] });
    if (ok) useStore.getState().toast({ kind: "success", text: `Imported ${ok === 1 ? list[0].name : `${ok} instances`}` });
  };

  return (
    <div className="flex flex-col gap-4 p-5">
      {discovered.isLoading ? (
        <p className="flex items-center gap-2 text-[13px] text-ink-3">
          <span className="spinner" /> Looking for other launchers…
        </p>
      ) : groups.length === 0 ? (
        <p className="text-[13px] text-ink-2">No Prism Launcher, MultiMC, CurseForge App or Modrinth App instances in their usual places. If yours are elsewhere, choose the folder.</p>
      ) : (
        <div className="flex max-h-[46vh] flex-col gap-3 overflow-y-auto pr-1">
          {groups.map(([from, list]) => (
            <div key={from}>
              <p className="mb-1.5 text-[12px] font-medium text-ink-3">{launcherName[from]}</p>
              <ul className="flex flex-col gap-1">
                {list.map((f, i) => (
                  <motion.li key={f.gameDir} initial={{ opacity: 0, y: 4 }} animate={{ opacity: 1, y: 0, transition: { delay: Math.min(i, 10) * 0.025 } }}>
                    <label className="flex cursor-pointer items-center gap-3 rounded-[8px] bg-slate-900/70 px-3 py-2.5 transition-colors hover:bg-slate-800">
                      <input type="checkbox" className="accent-[var(--color-gold)]" checked={selected.has(f.gameDir)} onChange={() => toggle(f.gameDir)} />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13.5px] font-medium text-ink">{f.name}</span>
                        <span className="block truncate text-[12px] text-ink-3">
                          Minecraft {f.gameVersion} · {loaderName[f.loader]}
                          {f.loaderVersion ? ` ${f.loaderVersion}` : ""} · {f.mods} mod{f.mods === 1 ? "" : "s"}
                          {f.worlds ? ` · ${f.worlds} world${f.worlds === 1 ? "" : "s"}` : ""}
                        </span>
                      </span>
                    </label>
                  </motion.li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
      <p className="text-[12px] text-ink-3">Alloy copies the whole game folder, worlds included. The other launcher's copy stays as it is.</p>
      <div className="flex items-center gap-2 border-t border-line-soft pt-4">
        <button className="btn btn-quiet" onClick={chooseFolder} disabled={!isTauri}>
          <FolderSearch size={14} /> Choose a folder
        </button>
        <span className="ml-auto" />
        <button className="btn btn-gold" disabled={busy || selected.size === 0} onClick={importAll}>
          Import {selected.size > 1 ? `${selected.size} instances` : ""}
        </button>
      </div>
    </div>
  );
}
