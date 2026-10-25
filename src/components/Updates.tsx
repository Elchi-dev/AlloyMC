// Updates in the UI: finds verified `.alloyupdate` files, offers them with one click, and
// reports the result after the restart. All checks (signatures, hash, platform) are in Rust.

import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ArrowUpCircle, X } from "lucide-react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, isTauri } from "../lib/api";
import { toastError, useStore } from "../lib/store";

const CHECK_EVERY = 60_000;

/** Mount once: reports the last update, then looks for new ones on start, on focus and on drop. */
export function useUpdates() {
  const setUpdate = useStore((s) => s.setUpdate);
  useEffect(() => {
    let last = 0;
    const check = () => {
      if (Date.now() - last < CHECK_EVERY) return;
      last = Date.now();
      api.checkUpdates().then((u) => {
        const prev = useStore.getState().update;
        setUpdate(u);
        if (u && prev?.manifest.version !== u.manifest.version) {
          useStore.getState().toast({ kind: "info", text: `Alloy ${u.manifest.version} is ready to install` });
        }
      }).catch(() => {});
    };

    api.updateOutcome().then((o) => {
      if (!o) return;
      useStore.getState().toast(
        o.ok
          ? { kind: "success", text: `Updated to Alloy ${o.to}` }
          : { kind: "error", text: `The update to ${o.to} didn't finish. Your data is untouched; try it again from Settings.` },
      );
    }).catch(() => {});
    check();
    window.addEventListener("focus", check);

    // Dropping an .alloyupdate file anywhere on the window offers it.
    let unlisten: (() => void) | undefined;
    if (isTauri) {
      getCurrentWebview()
        .onDragDropEvent((e) => {
          if (e.payload.type !== "drop") return;
          const file = e.payload.paths.find((p) => p.endsWith(".alloyupdate"));
          if (!file) return;
          api.inspectUpdate(file).then((u) => {
            setUpdate(u);
            useStore.getState().toast({
              kind: u.newer ? "info" : "error",
              text: u.newer ? `Alloy ${u.manifest.version} is verified and ready` : `This is Alloy ${u.manifest.version}; you already have ${u.current}`,
            });
          }).catch(toastError);
        })
        .then((f) => (unlisten = f));
    }
    return () => {
      window.removeEventListener("focus", check);
      unlisten?.();
    };
  }, [setUpdate]);
}

export async function applyUpdate(path: string, allowDowngrade = false) {
  try {
    useStore.getState().toast({ kind: "info", text: "Installing the update. Alloy restarts in a moment." });
    await api.applyUpdate(path, allowDowngrade);
  } catch (e) {
    toastError(e);
  }
}

/** Sidebar button that appears only when an update is ready. One click to install. */
export function UpdateButton() {
  const update = useStore((s) => s.update);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  return (
    <AnimatePresence>
      {update?.newer && (
        <motion.div
          ref={ref}
          className="relative"
          initial={{ scale: 0, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          exit={{ scale: 0, opacity: 0 }}
          transition={{ type: "spring", stiffness: 500, damping: 26 }}
        >
          <button
            onClick={() => setOpen((v) => !v)}
            aria-label={`Update to Alloy ${update.manifest.version}`}
            className="relative grid h-11 w-11 place-items-center rounded-[8px] text-gold transition-transform hover:bg-slate-850 active:scale-90"
          >
            <span className="update-ring absolute inset-1.5 rounded-full border border-gold/60" aria-hidden />
            <ArrowUpCircle size={20} />
          </button>
          <AnimatePresence>
            {open && (
              <motion.div
                initial={{ opacity: 0, x: -6, scale: 0.98 }}
                animate={{ opacity: 1, x: 0, scale: 1 }}
                exit={{ opacity: 0, x: -6, scale: 0.98 }}
                transition={{ duration: 0.14 }}
                className="absolute bottom-0 left-[56px] z-50 w-80 origin-bottom-left rounded-[10px] border border-line bg-slate-850 p-4 shadow-2xl shadow-black/50"
              >
                <div className="flex items-start">
                  <div>
                    <p className="text-[15px] font-semibold">Alloy {update.manifest.version} is ready</p>
                    <p className="text-[12px] text-ink-3">You have {update.current}. Signature verified.</p>
                  </div>
                  <button className="ml-auto text-ink-3 hover:text-ink" onClick={() => setOpen(false)} aria-label="Close">
                    <X size={14} />
                  </button>
                </div>
                {update.manifest.notes && (
                  <pre className="mt-3 max-h-40 overflow-auto whitespace-pre-wrap font-sans text-[12px] leading-relaxed text-ink-2" data-selectable>
                    {update.manifest.notes}
                  </pre>
                )}
                <button
                  className="btn btn-gold mt-4 w-full justify-center"
                  disabled={busy}
                  onClick={() => {
                    setBusy(true);
                    applyUpdate(update.path).finally(() => setBusy(false));
                  }}
                >
                  {busy ? <><span className="spinner" /> Installing</> : "Restart and update"}
                </button>
                <p className="mt-2 text-center text-[11px] text-ink-3">Your instances, worlds and accounts are kept. Alloy backs up its settings first.</p>
              </motion.div>
            )}
          </AnimatePresence>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
