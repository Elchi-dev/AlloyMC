// Problems the pre-launch check finds, with one-click fixes. Used by the dialog that opens
// when Play finds a problem, and by the banner on the Content tab.

import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { AlertTriangle, Check, CircleAlert, Play, Wrench } from "lucide-react";
import { api } from "../lib/api";
import { useInstance } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { Fix, InstallOutcome, Problem, Suggestion } from "../lib/types";
import { Dialog } from "./ui";
import { Ori } from "./ori/Ori";

/** Fix lookups hit Modrinth and CurseForge, so they're fetched once and shared. */
export function useFixes(instanceId: string, enabled: boolean) {
  return useQuery({ queryKey: ["fixes", instanceId], queryFn: () => api.fixes(instanceId), enabled, staleTime: 60_000 });
}

function fixKey(f: Fix) {
  return JSON.stringify(f);
}

export function reportOutcome(out: InstallOutcome) {
  const t = useStore.getState().toast;
  if (out.installed.length) t({ kind: "success", text: `Installed ${out.installed.join(", ")}` });
  if (out.unresolved.length) t({ kind: "error", text: `No compatible version found for ${out.unresolved.join(", ")}` });
  if (out.blocked.length) t({ kind: "info", text: `${out.blocked.length} file${out.blocked.length > 1 ? "s need" : " needs"} a manual download from CurseForge; see the Content tab.` });
}

export function ProblemList({ instanceId, problems, onFixed }: { instanceId: string; problems: Problem[]; onFixed?: () => void }) {
  const qc = useQueryClient();
  const fixes = useFixes(instanceId, problems.length > 0);
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [done, setDone] = useState<Set<string>>(new Set());
  const forProblem = (p: Problem) => fixes.data?.filter((s) => s.path === p.path && s.message === p.message) ?? [];

  const apply = async (s: Suggestion) => {
    const k = fixKey(s.fix);
    setRunning((r) => new Set(r).add(k));
    try {
      reportOutcome(await api.applyFix(instanceId, s.fix));
      setDone((d) => new Set(d).add(k));
      await qc.invalidateQueries({ queryKey: ["fixes", instanceId] });
      onFixed?.();
    } catch (e) {
      toastError(e);
    } finally {
      setRunning((r) => {
        const n = new Set(r);
        n.delete(k);
        return n;
      });
    }
  };

  // One "Install Cloth Config" can fix three problems; offer each fix once.
  const shown = useMemo(() => new Set(problems.map((p) => `${p.path}|${p.message}`)), [problems]);
  const unique = useMemo(() => {
    const seen = new Map<string, Suggestion>();
    fixes.data?.filter((s) => shown.has(`${s.path}|${s.message}`)).forEach((s) => seen.set(fixKey(s.fix), s));
    return [...seen.values()];
  }, [fixes.data, shown]);
  // Disabling is a judgement call; "fix all" only installs, updates and removes duplicates.
  const automatic = unique.filter((s) => s.fix.action !== "disable" && !done.has(fixKey(s.fix)));
  const fixAll = async () => {
    for (const s of automatic) await apply(s);
  };

  return (
    <div className="flex flex-col gap-1.5">
      {problems.map((p, i) => (
        <motion.div
          key={`${p.path}-${p.message}`}
          layout
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0, transition: { delay: Math.min(i, 8) * 0.035 } }}
          className="flex items-start gap-3 rounded-[8px] bg-slate-900/80 px-3 py-2.5"
        >
          {p.severity === "error" ? <CircleAlert size={16} className="mt-0.5 shrink-0 text-redstone" /> : <AlertTriangle size={16} className="mt-0.5 shrink-0 text-gold" />}
          <p className="min-w-0 flex-1 text-[13px] leading-snug text-ink" data-selectable>
            {p.message}
          </p>
          <div className="flex shrink-0 flex-wrap justify-end gap-1.5">
            {fixes.isLoading && <span className="skeleton h-7 w-24 rounded-[6px]" />}
            {forProblem(p).map((s) => {
              const k = fixKey(s.fix);
              const install = s.fix.action === "installModrinth" || s.fix.action === "installCurseForge";
              return (
                <button key={k} className={`btn h-7 px-2.5 text-[12px] ${install ? "btn-gold" : ""}`} disabled={running.has(k) || done.has(k)} onClick={() => apply(s)}>
                  {done.has(k) ? <Check size={13} className="pop-in" /> : running.has(k) ? <span className="spinner" /> : null}
                  {s.label}
                </button>
              );
            })}
            {fixes.isSuccess && forProblem(p).length === 0 && p.type === "missing" && (
              <span className="pt-1 text-[12px] text-ink-3">Not on Modrinth{" "}or CurseForge for this version</span>
            )}
          </div>
        </motion.div>
      ))}
      {automatic.length > 1 && (
        <div className="flex justify-end pt-1">
          <button className="btn btn-gold" onClick={fixAll} disabled={running.size > 0}>
            <Wrench size={14} /> Apply {automatic.length} fixes
          </button>
        </div>
      )}
    </div>
  );
}

/** Opens when Play finds something that would stop the game. */
export function PreflightDialog() {
  const pf = useStore((s) => s.preflight);
  const setPf = useStore((s) => s.setPreflight);
  const go = useStore((s) => s.go);
  const { data: instance } = useInstance(pf?.instanceId ?? "");
  const [checking, setChecking] = useState(false);
  const close = () => setPf(null);
  const recheck = async () => {
    if (!pf) return;
    setChecking(true);
    try {
      const result = await api.preflight(pf.instanceId);
      setPf({ instanceId: pf.instanceId, result });
    } finally {
      setChecking(false);
    }
  };
  const play = async () => {
    if (!pf) return;
    const id = pf.instanceId;
    close();
    try {
      await api.launch(id);
    } catch (e) {
      toastError(e);
    }
  };
  const errors = pf?.result.errors ?? [];
  const clean = !!pf && errors.length === 0 && pf.result.system.length === 0;
  return (
    <Dialog open={!!pf} onClose={close} title={clean ? "All fixed" : "Before you play"} width={640}>
      {pf && (
        <div className="flex flex-col gap-4 p-5">
          <div className="flex items-center gap-4">
            <Ori pose={clean ? "done" : "oops"} size={64} />
            <p className="text-[14px] text-ink-2">
              {clean ? (
                <>Everything checks out. {instance?.name} is ready to start.</>
              ) : (
                <>
                  Alloy found {errors.length + pf.result.system.length === 1 ? "a problem" : `${errors.length + pf.result.system.length} problems`} that would stop{" "}
                  <span className="font-semibold text-ink">{instance?.name}</span> from starting. It checked {errors.length ? "the mods' own requirements" : "your setup"} instead of waiting for a crash.
                </>
              )}
            </p>
          </div>
          <AnimatePresence>
            {pf.result.system.map((s) => (
              <motion.p key={s} initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="rounded-[8px] bg-slate-900/80 px-3 py-2.5 text-[13px]" data-selectable>
                {s}
              </motion.p>
            ))}
          </AnimatePresence>
          {errors.length > 0 && (
            <div className="max-h-[46vh] overflow-y-auto pr-1">
              <ProblemList instanceId={pf.instanceId} problems={errors} onFixed={recheck} />
            </div>
          )}
          <div className="flex items-center gap-2 border-t border-line-soft pt-4">
            {!clean && (
              <button className="btn btn-quiet" onClick={() => (close(), go({ name: "instance", id: pf.instanceId, tab: "content" }))}>
                Open content
              </button>
            )}
            <span className="ml-auto" />
            {!clean && (
              <button className="btn" onClick={recheck} disabled={checking}>
                {checking ? <span className="spinner" /> : null} Check again
              </button>
            )}
            <button className={`btn ${clean ? "btn-gold" : ""}`} onClick={play}>
              <Play size={14} fill="currentColor" strokeWidth={0} /> {clean ? "Play" : "Play anyway"}
            </button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
