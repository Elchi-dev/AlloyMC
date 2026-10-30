// The downloads panel: every install, import and update as a job, with live progress,
// the overall speed, cancel while running and retry after a failure.

import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Check, Download, RotateCw, X } from "lucide-react";
import { api } from "../lib/api";
import { bytes, progressFraction, progressLabel } from "../lib/format";
import { toastError, useStore } from "../lib/store";
import type { Job } from "../lib/types";
import { Ori } from "./ori/Ori";

function useClickOutside(open: boolean, close: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && close();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, close]);
  return ref;
}

/** Overall progress of the running jobs, 0 to 1 (jobs without numbers count as half done). */
function overall(jobs: Job[]): number {
  const running = jobs.filter((j) => j.state === "running");
  if (!running.length) return 0;
  const sum = running.reduce((a, j) => a + (j.progress && j.progress.total ? progressFraction(j.progress) : 0.5), 0);
  return sum / running.length;
}

export function DownloadsButton() {
  const jobs = useStore((s) => s.jobList);
  const [open, setOpen] = useState(false);
  const [seenFailures, setSeenFailures] = useState(0);
  const ref = useClickOutside(open, () => setOpen(false));
  const running = jobs.filter((j) => j.state === "running").length;
  const failed = jobs.filter((j) => j.state === "failed").length;
  const fraction = overall(jobs);
  const unseenFailure = failed > seenFailures && !open;
  useEffect(() => {
    if (open) setSeenFailures(failed);
  }, [open, failed]);

  if (!jobs.length) return null;
  const r = 19;
  const circ = 2 * Math.PI * r;
  return (
    <div ref={ref} className="relative">
      <motion.button
        initial={{ scale: 0 }}
        animate={{ scale: 1 }}
        transition={{ type: "spring", stiffness: 500, damping: 26 }}
        onClick={() => setOpen((v) => !v)}
        aria-label={running ? `${running} running, open downloads` : "Open downloads"}
        aria-expanded={open}
        className={`group relative grid h-11 w-11 place-items-center rounded-[8px] transition-[color,background,transform] duration-150 active:scale-90 ${open ? "bg-slate-800 text-ink" : "text-ink-3 hover:bg-slate-850 hover:text-ink"}`}
      >
        <svg className="absolute inset-0 -rotate-90" viewBox="0 0 44 44" aria-hidden>
          {running > 0 && (
            <>
              <circle cx="22" cy="22" r={r} fill="none" stroke="var(--color-line)" strokeWidth="2" />
              <circle
                cx="22"
                cy="22"
                r={r}
                fill="none"
                stroke="var(--color-gold)"
                strokeWidth="2"
                strokeLinecap="round"
                strokeDasharray={circ}
                strokeDashoffset={circ * (1 - Math.max(0.04, fraction))}
                className="transition-[stroke-dashoffset] duration-300"
              />
            </>
          )}
        </svg>
        <Download size={18} className={`relative transition-transform duration-200 group-hover:scale-110 ${running ? "download-bob text-gold" : ""}`} />
        {unseenFailure && <span className="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-redstone" aria-label="A download failed" />}
      </motion.button>
      <AnimatePresence>{open && <Panel jobs={jobs} />}</AnimatePresence>
    </div>
  );
}

function Panel({ jobs }: { jobs: Job[] }) {
  const speed = useStore((s) => s.speed);
  const running = jobs.filter((j) => j.state === "running");
  const ordered = [...jobs].sort((a, b) => Number(b.state === "running") - Number(a.state === "running") || b.startedAt - a.startedAt);
  return (
    <motion.div
      initial={{ opacity: 0, x: -6, scale: 0.98 }}
      animate={{ opacity: 1, x: 0, scale: 1 }}
      exit={{ opacity: 0, x: -6, scale: 0.98, transition: { duration: 0.12 } }}
      transition={{ type: "spring", stiffness: 520, damping: 36 }}
      className="absolute bottom-0 left-[56px] z-50 flex max-h-[70vh] w-[380px] origin-bottom-left flex-col rounded-[12px] border border-line bg-slate-850 shadow-2xl shadow-black/50"
    >
      <div className="flex items-center gap-2 border-b border-line-soft px-4 py-3">
        <h2 className="text-[14px] font-semibold">Downloads</h2>
        <AnimatePresence>
          {running.length > 0 && (
            <motion.span initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="tabular-nums text-[12px] text-ink-3">
              {bytes(speed)}/s
            </motion.span>
          )}
        </AnimatePresence>
        <button className="ml-auto text-[12px] text-ink-3 transition-colors hover:text-ink disabled:opacity-40" disabled={running.length === jobs.length} onClick={() => api.clearJobs().catch(toastError)}>
          Clear finished
        </button>
      </div>
      <div className="min-h-0 overflow-y-auto p-1.5">
        {ordered.length === 0 ? (
          <div className="flex items-center gap-3 px-3 py-4 text-[13px] text-ink-3">
            <Ori pose="sleeping" size={48} /> Nothing is downloading.
          </div>
        ) : (
          <AnimatePresence initial={false}>
            {ordered.map((j) => (
              <JobRow key={j.id} job={j} />
            ))}
          </AnimatePresence>
        )}
      </div>
    </motion.div>
  );
}

function JobRow({ job }: { job: Job }) {
  const go = useStore((s) => s.go);
  const p = job.progress;
  const fraction = p && p.total ? progressFraction(p) : null;
  const detail =
    job.state === "running"
      ? p
        ? `${progressLabel(p)}${p.total > 1 && p.unit === "files" ? ` ${p.current.toLocaleString()} of ${p.total.toLocaleString()}` : ""}`
        : "Starting"
      : job.state === "done"
        ? "Done"
        : job.state === "cancelled"
          ? "Cancelled"
          : job.error ?? "Failed";
  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, x: 24, transition: { duration: 0.14 } }}
      transition={{ type: "spring", stiffness: 500, damping: 38 }}
      className="group rounded-[8px] px-3 py-2.5 transition-colors hover:bg-slate-800"
    >
      <div className="flex items-center gap-2.5">
        <StateIcon state={job.state} />
        <button
          className="min-w-0 flex-1 text-left disabled:cursor-default"
          disabled={!job.instanceId}
          onClick={() => job.instanceId && go({ name: "instance", id: job.instanceId })}
        >
          <span className="block truncate text-[13px] font-medium">{job.title}</span>
          <span className={`block truncate text-[12px] ${job.state === "failed" ? "text-redstone" : "text-ink-3"}`} title={detail}>
            {detail}
          </span>
        </button>
        {job.state === "running" && (
          <button aria-label={`Cancel ${job.title}`} onClick={() => api.cancelJob(job.id).catch(toastError)} className="grid h-7 w-7 place-items-center rounded-[6px] text-ink-3 transition-[color,background,transform] hover:bg-slate-700 hover:text-redstone active:scale-90">
            <X size={14} />
          </button>
        )}
        {job.canRetry && job.state !== "running" && (
          <button aria-label={`Retry ${job.title}`} onClick={() => api.retryJob(job.id).catch(toastError)} className="grid h-7 w-7 place-items-center rounded-[6px] text-ink-3 transition-[color,background,transform] hover:bg-slate-700 hover:text-gold active:scale-90 active:rotate-180">
            <RotateCw size={14} />
          </button>
        )}
      </div>
      {job.state === "running" && (
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-slate-700">
          {fraction == null ? (
            <div className="indeterminate h-full w-1/3 rounded-full bg-gold" />
          ) : (
            <div className="h-full rounded-full bg-gold transition-[width] duration-300" style={{ width: `${Math.max(3, fraction * 100)}%` }} />
          )}
        </div>
      )}
    </motion.div>
  );
}

function StateIcon({ state }: { state: Job["state"] }) {
  if (state === "running") return <span className="spinner shrink-0" aria-hidden />;
  if (state === "done") return <Check size={15} className="pop-in shrink-0 text-grass" aria-hidden />;
  if (state === "cancelled") return <X size={15} className="shrink-0 text-ink-3" aria-hidden />;
  return <span className="grid h-[15px] w-[15px] shrink-0 place-items-center rounded-full bg-redstone text-[10px] font-bold text-white" aria-hidden>!</span>;
}
