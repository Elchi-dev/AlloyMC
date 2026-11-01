// The Save points tab: copies of an instance's setup to go back to.

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { Bookmark, History, Pencil, RotateCcw, Trash2 } from "lucide-react";
import { api } from "../lib/api";
import { ago, bytes, loaderName } from "../lib/format";
import { toastError, useStore } from "../lib/store";
import type { Instance, SavePoint } from "../lib/types";
import { Dialog, Empty } from "./ui";

export function SavePointsTab({ instance }: { instance: Instance }) {
  const { data } = useQuery({ queryKey: ["savepoints", instance.id], queryFn: () => api.savepoints(instance.id) });
  const running = useStore((s) => !!s.game[instance.id]);
  const [label, setLabel] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<SavePoint | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  const act = async (key: string, f: () => Promise<unknown>, done?: string) => {
    setBusy(key);
    try {
      await f();
      if (done) useStore.getState().toast({ kind: "success", text: done });
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(null);
    }
  };
  const make = (e: React.FormEvent) => {
    e.preventDefault();
    act("new", () => api.savepointCreate(instance.id, label.trim() || "Save point"), "Save point made").then(() => setLabel(""));
  };
  const restore = (p: SavePoint) => {
    setConfirm(null);
    act(p.id, () => api.savepointRestore(instance.id, p.id), `Back to "${p.label}"`);
  };
  const points = data?.points ?? [];

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-5 px-8 py-6">
      <form onSubmit={make} className="panel rise flex flex-col gap-3 rounded-[12px] p-5" style={{ ["--i" as string]: 0 } as React.CSSProperties}>
        <div>
          <h3 className="text-[15px] font-semibold">Make a save point</h3>
          <p className="mt-1 text-[13px] text-ink-3">
            A copy of the mods, configs, resource packs, shaders and settings, to go back to if an update or an experiment goes wrong. Worlds aren't included. Alloy also makes one before updating mods.
          </p>
        </div>
        <div className="flex gap-2">
          <input className="field flex-1" value={label} onChange={(e) => setLabel(e.target.value)} placeholder="What's about to change? (optional)" maxLength={80} aria-label="Save point name" />
          <button className="btn btn-gold" disabled={busy === "new"}>
            {busy === "new" ? <span className="spinner" /> : <Bookmark size={14} />} Save
          </button>
        </div>
      </form>

      {points.length === 0 ? (
        <Empty pose="sleeping" title="No save points yet" body="Make one before you try something risky. They take little space: files that didn't change are stored once." />
      ) : (
        <div className="flex flex-col gap-2">
          <p className="flex items-center gap-2 text-[12px] text-ink-3">
            <History size={13} /> {points.length} save point{points.length > 1 ? "s" : ""}, {bytes(data?.diskBytes ?? 0)} on disk
          </p>
          <ul className="flex flex-col gap-2">
            <AnimatePresence initial={false}>
              {points.map((p, i) => {
                const changedVersion = p.gameVersion !== instance.gameVersion || p.loader !== instance.loader || (p.loaderVersion ?? null) !== (instance.loaderVersion ?? null);
                return (
                  <motion.li
                    key={p.id}
                    layout
                    initial={{ opacity: 0, y: 8 }}
                    animate={{ opacity: 1, y: 0, transition: { delay: Math.min(i, 8) * 0.03 } }}
                    exit={{ opacity: 0, height: 0, marginTop: -8 }}
                    className="panel flex items-center gap-4 rounded-[10px] px-4 py-3"
                  >
                    <div className="min-w-0 flex-1">
                      {editing === p.id ? (
                        <form
                          onSubmit={(e) => {
                            e.preventDefault();
                            setEditing(null);
                            if (draft.trim() && draft.trim() !== p.label) act(`r:${p.id}`, () => api.savepointRename(instance.id, p.id, draft.trim()));
                          }}
                        >
                          <input autoFocus className="field h-8" value={draft} onChange={(e) => setDraft(e.target.value)} onBlur={() => setEditing(null)} maxLength={80} aria-label="New name" />
                        </form>
                      ) : (
                        <p className="flex items-center gap-2 truncate text-[14px] font-medium text-ink">
                          {p.label}
                          {p.auto && <span className="rounded-[4px] bg-slate-700 px-1.5 py-0.5 text-[10.5px] font-semibold text-ink-2">Automatic</span>}
                        </p>
                      )}
                      <p className="mt-0.5 text-[12px] text-ink-3">
                        {ago(p.createdAt)} · {p.mods} mod{p.mods === 1 ? "" : "s"} · {bytes(p.size)}
                        {changedVersion && (
                          <span className="text-molten">
                            {" "}
                            · Minecraft {p.gameVersion}, {loaderName[p.loader]}
                            {p.loaderVersion ? ` ${p.loaderVersion}` : ""}
                          </span>
                        )}
                      </p>
                    </div>
                    <button className="btn btn-quiet h-8 w-8 p-0" aria-label={`Rename ${p.label}`} onClick={() => (setDraft(p.label), setEditing(p.id))}>
                      <Pencil size={13} />
                    </button>
                    <button className="btn btn-quiet h-8 w-8 p-0" aria-label={`Delete ${p.label}`} disabled={busy === `d:${p.id}`} onClick={() => act(`d:${p.id}`, () => api.savepointDelete(instance.id, p.id))}>
                      <Trash2 size={13} />
                    </button>
                    <button className="btn" disabled={running || busy === p.id} onClick={() => setConfirm(p)} title={running ? "Close the game first" : undefined}>
                      {busy === p.id ? <span className="spinner" /> : <RotateCcw size={13} />} Go back
                    </button>
                  </motion.li>
                );
              })}
            </AnimatePresence>
          </ul>
        </div>
      )}

      <Dialog open={!!confirm} onClose={() => setConfirm(null)} title="Go back to a save point" width={460}>
        {confirm && (
          <div className="flex flex-col gap-4 p-5">
            <p className="text-[14px] text-ink-2">
              {instance.name}'s mods, configs and settings go back to <b className="text-ink">{confirm.label}</b> from {ago(confirm.createdAt).toLowerCase()}. Worlds stay as they are.
            </p>
            <p className="text-[13px] text-ink-3">Alloy saves the current state first, so you can come back to it.</p>
            <div className="flex justify-end gap-2 border-t border-line-soft pt-4">
              <button className="btn btn-quiet" onClick={() => setConfirm(null)}>
                Cancel
              </button>
              <button className="btn btn-gold" onClick={() => restore(confirm)}>
                <RotateCcw size={13} /> Go back
              </button>
            </div>
          </div>
        )}
      </Dialog>
    </div>
  );
}
