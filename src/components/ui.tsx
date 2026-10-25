import { useEffect, useId, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { X } from "lucide-react";
import { useStore } from "../lib/store";
import { Ori, type OriPose } from "./ori/Ori";

const spring = { type: "spring" as const, stiffness: 520, damping: 36 };

export function Dialog({ open, onClose, title, children, width = 520 }: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  width?: number;
}) {
  useEffect(() => {
    if (!open) return;
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", esc);
    return () => window.removeEventListener("keydown", esc);
  }, [open, onClose]);
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-40 grid place-items-center bg-black/60 p-6"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0, transition: { duration: 0.14 } }}
          transition={{ duration: 0.18 }}
          onMouseDown={(e) => e.target === e.currentTarget && onClose()}
        >
          <motion.div
            role="dialog"
            aria-modal
            aria-label={title}
            initial={{ y: 18, scale: 0.96, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1, transition: { type: "spring", stiffness: 420, damping: 30 } }}
            exit={{ y: 8, scale: 0.98, opacity: 0, transition: { duration: 0.14, ease: [0.55, 0, 0.75, 0.1] } }}
            className="w-full rounded-[12px] border border-line bg-slate-850 shadow-2xl shadow-black/60"
            style={{ maxWidth: width }}
          >
            <div className="flex items-center border-b border-line-soft px-5 py-3.5">
              <h2 className="text-[15px] font-semibold">{title}</h2>
              <button onClick={onClose} aria-label="Close" className="btn btn-quiet ml-auto h-7 px-1.5 hover:rotate-90">
                <X size={16} />
              </button>
            </div>
            {children}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** Options with a pill that slides to the selected one. */
export function Segmented<T extends string>({ value, onChange, options, label }: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: ReactNode }[];
  label: string;
}) {
  const id = useId();
  return (
    <div role="radiogroup" aria-label={label} className="inline-flex rounded-[8px] border border-line bg-slate-900/90 p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={`relative h-7 rounded-[6px] px-3 text-[13px] font-medium transition-colors duration-150 active:scale-95 ${
            value === o.value ? "text-ink" : "text-ink-3 hover:text-ink"
          }`}
        >
          {value === o.value && <motion.span layoutId={`seg-${id}`} className="absolute inset-0 rounded-[6px] bg-slate-700 shadow-sm" transition={spring} />}
          <span className="relative">{o.label}</span>
        </button>
      ))}
    </div>
  );
}

/** Tabs with an underline that slides between them. */
export function Tabs<T extends string>({ value, onChange, options }: { value: T; onChange: (v: T) => void; options: { value: T; label: string }[] }) {
  const id = useId();
  return (
    <div className="flex gap-1" role="tablist">
      {options.map((o) => (
        <button
          key={o.value}
          role="tab"
          aria-selected={value === o.value}
          onClick={() => onChange(o.value)}
          className={`relative px-3 pb-3 pt-1 text-[13px] font-medium transition-colors duration-150 ${value === o.value ? "text-ink" : "text-ink-3 hover:text-ink-2"}`}
        >
          {o.label}
          {value === o.value && <motion.span layoutId={`tab-${id}`} className="absolute inset-x-3 bottom-0 h-[2px] rounded bg-gold" transition={spring} />}
        </button>
      ))}
    </div>
  );
}

/** On/off switch whose knob springs across. */
export function Switch({ on, onChange, label }: { on: boolean; onChange: () => void; label: string }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      aria-label={label}
      onClick={onChange}
      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors duration-200 ${on ? "bg-gold" : "bg-slate-700"}`}
    >
      <motion.span
        className="absolute top-0.5 h-4 w-4 rounded-full bg-white shadow"
        animate={{ left: on ? 18 : 2 }}
        transition={{ type: "spring", stiffness: 700, damping: 30 }}
      />
    </button>
  );
}

const toastPose: Record<string, OriPose> = { error: "oops", success: "done", info: "thinking" };

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  const dismiss = useStore((s) => s.dismiss);
  return (
    <div className="pointer-events-none fixed bottom-5 right-5 z-50 flex w-96 flex-col gap-2" aria-live="polite">
      <AnimatePresence initial={false}>
        {toasts.map((t) => (
          <motion.div
            key={t.id}
            layout
            initial={{ opacity: 0, y: 16, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1, transition: { type: "spring", stiffness: 480, damping: 32 } }}
            exit={{ opacity: 0, x: 40, transition: { duration: 0.16, ease: [0.55, 0, 0.75, 0.1] } }}
            className="pointer-events-auto relative flex items-center gap-3 overflow-hidden rounded-[10px] border border-line bg-slate-800/95 py-2.5 pl-2 pr-4 shadow-xl shadow-black/40"
          >
            <Ori pose={toastPose[t.kind]} size={36} />
            <p className="flex-1 text-[13px] leading-snug" data-selectable>
              {t.text}
            </p>
            {t.action && (
              <button className="text-[13px] font-semibold text-gold transition-opacity hover:opacity-80" onClick={() => { t.action!.run(); dismiss(t.id); }}>
                {t.action.label}
              </button>
            )}
            <button onClick={() => dismiss(t.id)} aria-label="Dismiss" className="text-ink-3 transition-transform hover:rotate-90 hover:text-ink">
              <X size={14} />
            </button>
            <span
              className={`toast-timer absolute bottom-0 left-0 h-[2px] ${t.kind === "error" ? "bg-redstone" : t.kind === "success" ? "bg-grass" : "bg-gold"}`}
              style={{ animationDuration: t.kind === "error" ? "8s" : "4s" }}
            />
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}

export function Empty({ title, body, action, pose = "hi" }: { title: string; body: string; action?: ReactNode; pose?: OriPose }) {
  return (
    <div className="rise flex items-center gap-5 rounded-[12px] border border-dashed border-line bg-slate-900/70 px-6 py-6">
      <Ori pose={pose} size={84} />
      <div className="flex flex-col items-start gap-1.5">
        <h3 className="text-[15px] font-semibold">{title}</h3>
        <p className="max-w-md text-ink-2">{body}</p>
        {action && <div className="mt-2">{action}</div>}
      </div>
    </div>
  );
}
