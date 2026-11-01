// The crash doctor: opens when the game crashes, says what happened and which mod it points
// at, offers the fixes, and can find the mod by starting the game with halves of the mods off.

import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { ChevronDown, FileText, FlaskConical, Play, PowerOff, ScrollText, Sparkles, X } from "lucide-react";
import { useAssistant } from "../lib/assistant";
import { api } from "../lib/api";
import { useInstance } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { BisectFound, Confidence, CrashFix, Diagnosis } from "../lib/types";
import { Dialog } from "./ui";
import { Ori } from "./ori/Ori";

const sureness: Record<Confidence, { label: string; cls: string }> = {
  high: { label: "Certain", cls: "bg-redstone/15 text-redstone" },
  medium: { label: "Likely", cls: "bg-molten/15 text-molten" },
  low: { label: "Possible", cls: "bg-slate-700 text-ink-2" },
};

function fixLabel(f: CrashFix) {
  switch (f.kind) {
    case "raiseMemory":
      return `Give it ${(f.mb / 1024).toFixed(f.mb % 1024 ? 1 : 0)} GB of memory`;
    case "useJava":
      return `Switch to Java ${f.major}`;
    case "disableMod":
      return "Turn it off";
  }
}

/** The last crash of an instance, for the banner on its page. */
export function useLastCrash(instanceId: string) {
  return useQuery({ queryKey: ["crash", instanceId], queryFn: () => api.crashLast(instanceId) });
}

export function CrashDoctorDialog() {
  const crash = useStore((s) => s.crash);
  const setCrash = useStore((s) => s.setCrash);
  const close = () => setCrash(null);
  return (
    <Dialog open={!!crash} onClose={close} title="Crash doctor" width={640}>
      {crash && <CrashDoctor instanceId={crash.instanceId} diagnosis={crash.diagnosis} onClose={close} />}
    </Dialog>
  );
}

function CrashDoctor({ instanceId, diagnosis: d, onClose }: { instanceId: string; diagnosis: Diagnosis; onClose: () => void }) {
  const qc = useQueryClient();
  const { data: instance } = useInstance(instanceId);
  const bisect = useStore((s) => s.bisect[instanceId]);
  const go = useStore((s) => s.go);
  const [busy, setBusy] = useState<string | null>(null);
  const [done, setDone] = useState<Set<string>>(new Set());
  const [details, setDetails] = useState(false);

  const apply = async (key: string, fix: CrashFix) => {
    setBusy(key);
    try {
      await api.crashApplyFix(instanceId, fix);
      setDone((s) => new Set(s).add(key));
      await qc.invalidateQueries({ queryKey: ["crash", instanceId] });
      useStore.getState().toast({ kind: "success", text: fix.kind === "disableMod" ? "Turned off" : "Changed. Try playing again." });
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(null);
    }
  };
  const startBisect = async () => {
    useStore.getState().setBisect(instanceId, { running: true });
    try {
      await api.bisectStart(instanceId);
    } catch (e) {
      useStore.getState().setBisect(instanceId, { running: false, error: String(e) });
    }
  };
  const cancelBisect = () => api.cancelJob(`bisect:${instanceId}`).catch(toastError);
  const play = async () => {
    onClose();
    await api.crashDismiss(instanceId).catch(() => {});
    qc.invalidateQueries({ queryKey: ["crash", instanceId] });
    api.launch(instanceId).catch(toastError);
  };

  const otherFixes = d.fixes.filter((f) => f.kind !== "disableMod");
  const pose = bisect?.running ? "searching" : bisect?.found?.culprits.length ? "done" : "oops";

  return (
    <div className="flex flex-col gap-4 p-5">
      <div className="flex items-start gap-4">
        <Ori pose={pose} size={64} />
        <div className="min-w-0 flex-1">
          <p className="text-[12px] font-medium uppercase tracking-wide text-ink-3">{instance?.name} crashed</p>
          <h3 className="mt-0.5 text-[17px] font-semibold text-ink">{d.title}</h3>
          <p className="mt-1.5 text-[13.5px] leading-relaxed text-ink-2">{d.detail}</p>
        </div>
      </div>

      {d.suspects.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <p className="text-[12px] font-medium text-ink-3">{d.suspects.length === 1 ? "The mod it points at" : "The mods it points at"}</p>
          <ul className="flex flex-col gap-1.5">
            {d.suspects.map((s, i) => {
              const key = `off:${s.path}`;
              const off = done.has(key);
              return (
                <motion.li
                  key={s.path}
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0, transition: { delay: 0.05 * i } }}
                  className="flex items-center gap-3 rounded-[8px] bg-slate-900/80 px-3 py-2.5"
                >
                  <span className={`shrink-0 rounded-[4px] px-1.5 py-0.5 text-[11px] font-semibold ${sureness[s.confidence].cls}`}>{sureness[s.confidence].label}</span>
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-[13.5px] font-medium text-ink">{s.name}</p>
                    <p className="truncate text-[12px] text-ink-3">{s.reason}</p>
                  </div>
                  <button className="btn btn-quiet shrink-0" disabled={off || busy === key} onClick={() => apply(key, { kind: "disableMod", path: s.path })}>
                    {busy === key ? <span className="spinner" /> : <PowerOff size={13} />} {off ? "Off" : "Turn off"}
                  </button>
                </motion.li>
              );
            })}
          </ul>
        </div>
      )}

      {otherFixes.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {otherFixes.map((f) => {
            const key = JSON.stringify(f);
            return (
              <button key={key} className="btn btn-gold" disabled={done.has(key) || busy === key} onClick={() => apply(key, f)}>
                {busy === key ? <span className="spinner" /> : null} {done.has(key) ? "Done" : fixLabel(f)}
              </button>
            );
          })}
        </div>
      )}

      {(d.canBisect || bisect) && <BisectPanel instanceId={instanceId} state={bisect} onStart={startBisect} onCancel={cancelBisect} apply={apply} busy={busy} done={done} hasSuspects={d.suspects.length > 0} />}

      {d.evidence.length > 0 && (
        <div>
          <button className="flex items-center gap-1.5 text-[12.5px] font-medium text-ink-2 transition-colors hover:text-ink" onClick={() => setDetails(!details)} aria-expanded={details}>
            <ChevronDown size={14} className={`transition-transform duration-200 ${details ? "rotate-180" : ""}`} /> What the logs say
          </button>
          <AnimatePresence initial={false}>
            {details && (
              <motion.pre
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                className="mt-2 max-h-48 overflow-auto rounded-[8px] bg-slate-950 px-3 py-2.5 font-mono text-[11.5px] leading-relaxed text-ink-2"
                data-selectable
              >
                {d.evidence.join("\n")}
              </motion.pre>
            )}
          </AnimatePresence>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2 border-t border-line-soft pt-4">
        <button className="btn btn-quiet" onClick={() => (onClose(), go({ name: "instance", id: instanceId, tab: "logs" }))}>
          <ScrollText size={14} /> Log
        </button>
        {d.reportPath && (
          <button className="btn btn-quiet" onClick={() => api.openFolder(instanceId, d.reportPath!.includes("hs_err") ? undefined : "crash-reports").catch(toastError)}>
            <FileText size={14} /> Crash report
          </button>
        )}
        <button
          className="btn btn-quiet"
          onClick={() => (onClose(), useAssistant.getState().ask("My game crashed. What happened, and how do I fix it?", instanceId))}
        >
          <Sparkles size={14} /> Ask the assistant
        </button>
        <span className="ml-auto" />
        <button className="btn btn-quiet" onClick={onClose}>
          Close
        </button>
        <button className="btn" onClick={play} disabled={bisect?.running}>
          <Play size={14} fill="currentColor" strokeWidth={0} /> Play again
        </button>
      </div>
    </div>
  );
}

function BisectPanel({
  state,
  onStart,
  onCancel,
  apply,
  busy,
  done,
  hasSuspects,
}: {
  instanceId: string;
  state?: { step?: { trial: number; estimate: number; enabled: number; suspects: number }; found?: BisectFound; error?: string; running: boolean };
  onStart: () => void;
  onCancel: () => void;
  apply: (key: string, fix: CrashFix) => void;
  busy: string | null;
  done: Set<string>;
  hasSuspects: boolean;
}) {
  const found = state?.found;
  return (
    <div className="rounded-[10px] border border-line-soft bg-slate-900/50 p-3.5">
      {!state && (
        <div className="flex items-start gap-3">
          <FlaskConical size={18} className="mt-0.5 shrink-0 text-gold" />
          <div className="flex-1">
            <p className="text-[13.5px] font-medium text-ink">{hasSuspects ? "Not sure? Test it" : "Find the mod by testing"}</p>
            <p className="mt-0.5 text-[12.5px] leading-relaxed text-ink-3">
              Alloy starts the game a few times with different halves of the mods turned off, and closes it at the title screen each time. Big packs take a few minutes per start. Your mods are put back afterwards.
            </p>
          </div>
          <button className="btn shrink-0" onClick={onStart}>
            Start
          </button>
        </div>
      )}
      {state?.running && (
        <div className="flex items-center gap-3">
          <span className="spinner" />
          <div className="min-w-0 flex-1">
            <p className="text-[13.5px] font-medium text-ink">{state.step ? `Start ${state.step.trial} of about ${state.step.estimate}` : "Getting ready"}</p>
            {state.step && (
              <>
                <p className="text-[12px] text-ink-3">
                  {state.step.enabled} mods on. It's one of {state.step.suspects}.
                </p>
                <div className="mt-2 h-1 overflow-hidden rounded-full bg-slate-800">
                  <motion.div className="h-full bg-gold" animate={{ width: `${Math.min(100, ((state.step.trial - 1) / state.step.estimate) * 100)}%` }} />
                </div>
              </>
            )}
          </div>
          <button className="btn btn-quiet shrink-0" onClick={onCancel}>
            <X size={13} /> Stop
          </button>
        </div>
      )}
      {state?.error && !state.running && (
        <p className="text-[13px] text-ink-2">
          Testing stopped: {state.error}. Your mods are as they were. <button className="font-medium text-gold hover:underline" onClick={onStart}>Try again</button>
        </p>
      )}
      {found && (
        <div className="flex flex-col gap-2">
          <p className="text-[13.5px] text-ink">
            {found.culprits.length === 0
              ? `With every mod on, the game started fine this time (${found.trials} starts). The crash may depend on something else, like a world or a server.`
              : found.alone
                ? <>It's <b>{found.names[0]}</b>. The game crashes with only it and what it needs turned on ({found.trials} starts).</>
                : <>It takes these mods together: <b>{found.names.join(" and ")}</b>. None of them crashes on its own ({found.trials} starts).</>}
          </p>
          {found.culprits.map((p, i) => {
            const key = `off:${p}`;
            return (
              <div key={p} className="flex items-center gap-3 rounded-[8px] bg-slate-900/80 px-3 py-2">
                <span className="flex-1 truncate text-[13px] text-ink">{found.names[i]}</span>
                <button className="btn btn-quiet" disabled={done.has(key) || busy === key} onClick={() => apply(key, { kind: "disableMod", path: p })}>
                  {busy === key ? <span className="spinner" /> : <PowerOff size={13} />} {done.has(key) ? "Off" : "Turn off"}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** A line on an instance's page while its last crash is unresolved. */
export function CrashBanner({ instanceId }: { instanceId: string }) {
  const { data } = useLastCrash(instanceId);
  const setCrash = useStore((s) => s.setCrash);
  const qc = useQueryClient();
  if (!data) return null;
  return (
    <motion.div initial={{ opacity: 0, y: -4 }} animate={{ opacity: 1, y: 0 }} className="flex items-center gap-3 rounded-[10px] border border-redstone/30 bg-redstone/10 px-3.5 py-2.5">
      <span className="h-2 w-2 shrink-0 rounded-full bg-redstone" />
      <p className="min-w-0 flex-1 truncate text-[13px] text-ink">
        Last start crashed: <span className="text-ink-2">{data.title}</span>
      </p>
      <button className="btn btn-quiet" onClick={() => setCrash({ instanceId, diagnosis: data })}>
        Open crash doctor
      </button>
      <button
        className="grid h-7 w-7 place-items-center rounded-[6px] text-ink-3 transition-colors hover:bg-slate-800 hover:text-ink"
        aria-label="Dismiss"
        onClick={async () => {
          await api.crashDismiss(instanceId).catch(() => {});
          qc.invalidateQueries({ queryKey: ["crash", instanceId] });
        }}
      >
        <X size={14} />
      </button>
    </motion.div>
  );
}
