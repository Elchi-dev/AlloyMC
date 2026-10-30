// Dropping files anywhere on the window: modpacks become instances, mods, resource packs and
// shaders go into the instance you're looking at, an image becomes its icon.

import { useEffect } from "react";
import { AnimatePresence, motion } from "motion/react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, isTauri } from "../lib/api";
import { useInstance } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { AddedFile } from "../lib/types";
import { InstanceIcon } from "./Art";

const IMAGE = /\.(png|jpe?g|webp|gif)$/i;

function summarize(results: AddedFile[], instanceName?: string) {
  const t = useStore.getState().toast;
  const ok = results.filter((r) => !r.error);
  const packs = ok.filter((r) => r.kind === "modpack");
  const content = ok.filter((r) => r.kind !== "modpack");
  if (packs.length) t({ kind: "info", text: `Importing ${packs.map((p) => p.name).join(", ")}. Follow it in the downloads panel.` });
  if (content.length) {
    const what = content.length === 1 ? content[0].name : `${content.length} files`;
    t({ kind: "success", text: `Added ${what}${instanceName ? ` to ${instanceName}` : ""}` });
  }
  results.filter((r) => r.error).forEach((r) => t({ kind: "error", text: `${r.name}: ${r.error}` }));
}

/** Mount once. */
export function useFileDrop() {
  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent(async (e) => {
        const s = useStore.getState();
        const p = e.payload;
        if (p.type === "enter") {
          // Update files have their own handler.
          if (p.paths.some((x) => !x.endsWith(".alloyupdate"))) s.setDropping(true);
          return;
        }
        if (p.type === "leave") return s.setDropping(false);
        if (p.type !== "drop") return;
        s.setDropping(false);
        const paths = p.paths.filter((x) => !x.endsWith(".alloyupdate"));
        if (!paths.length) return;
        const page = s.page;
        const instanceId = page.name === "instance" ? page.id : null;
        try {
          if (instanceId && paths.length === 1 && IMAGE.test(paths[0])) {
            s.setIconDrop({ instanceId, src: await api.readImageFile(paths[0]) });
            return;
          }
          const name = instanceId ? (await api.instance(instanceId)).name : undefined;
          summarize(await api.addFiles(instanceId, paths), name);
        } catch (err) {
          toastError(err);
        }
      })
      .then((f) => (unlisten = f));
    return () => unlisten?.();
  }, []);
}

export function DropOverlay() {
  const dropping = useStore((s) => s.dropping);
  const page = useStore((s) => s.page);
  const { data: instance } = useInstance(page.name === "instance" ? page.id : "");
  return (
    <AnimatePresence>
      {dropping && (
        <motion.div className="pointer-events-none fixed inset-0 z-[70] grid place-items-center bg-slate-950/70 p-6" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.15 }}>
          <div className="drop-frame absolute inset-4 rounded-[16px]" />
          <motion.div initial={{ scale: 0.94, y: 8 }} animate={{ scale: 1, y: 0 }} transition={{ type: "spring", stiffness: 420, damping: 28 }} className="flex flex-col items-center gap-3 text-center">
            {instance ? <InstanceIcon instance={instance} size={72} /> : null}
            <p className="font-pixel text-[26px]">{instance ? `Drop to add to ${instance.name}` : "Drop to import"}</p>
            <p className="max-w-md text-[13px] text-ink-2">
              {instance
                ? "Mods, resource packs and shaders go into this instance; modpacks become new instances; an image becomes its icon."
                : "Modpacks (.mrpack or CurseForge .zip) become new instances. Open an instance to drop mods into it."}
            </p>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
