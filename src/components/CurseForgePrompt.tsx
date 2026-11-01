// Asks once, right after an install, whether to fetch the files CurseForge only hands out on
// its website in Alloy's CurseForge window.

import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { Download } from "lucide-react";
import { api } from "../lib/api";
import { toastError, useStore } from "../lib/store";
import { Dialog } from "./ui";
import { Ori } from "./ori/Ori";

export function CurseForgePrompt() {
  const p = useStore((s) => s.curseforgePrompt);
  const close = () => useStore.getState().setCurseForgePrompt(null);
  const go = () => {
    if (!p) return;
    close();
    api.curseforgeWindow(p.instanceId).catch(toastError);
  };
  // What's still missing now: files already in Downloads are picked up on their own.
  const live = useQuery({ queryKey: ["blocked", p?.instanceId], queryFn: () => api.blockedFiles(p!.instanceId), enabled: !!p, refetchInterval: 4000 });
  const n = live.data?.length ?? p?.count ?? 0;
  useEffect(() => {
    if (p && live.data && live.data.length === 0) close();
  }, [p, live.data]);
  return (
    <Dialog open={!!p} onClose={close} title="Files from CurseForge's website" width={500}>
      <div className="flex flex-col gap-4 p-5">
        <div className="flex items-start gap-4">
          <Ori pose="needs" size={60} />
          <div className="text-[14px] text-ink-2">
            <p>
              {n === 1 ? "One mod's author" : `${n} mods' authors`} only allow downloads on CurseForge's own website, so Alloy can't fetch {n === 1 ? "it" : "them"} by
              itself.
            </p>
            <p className="mt-2">
              Alloy can open {n === 1 ? "its page" : "their pages one after another"} in a CurseForge window. CurseForge starts each download after a few seconds; Alloy
              checks the file, adds it and closes the window when {n === 1 ? "it's" : "they're all"} in.
            </p>
          </div>
        </div>
        <div className="flex justify-end gap-2 border-t border-line-soft pt-4">
          <button className="btn btn-quiet" onClick={close}>
            Later
          </button>
          <button className="btn btn-gold" onClick={go}>
            <Download size={14} /> Open CurseForge
          </button>
        </div>
      </div>
    </Dialog>
  );
}
