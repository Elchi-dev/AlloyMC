// Memory for the game: up to what this PC can spare, with a recommendation from the mod count.

import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { useMemoryAdvice } from "../lib/queries";

const STEP = 512;
const gb = (mb: number) => (mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1);

export function MemorySlider({ value, onChange, instanceId }: { value: number; onChange: (mb: number) => void; instanceId?: string }) {
  const { data: advice } = useMemoryAdvice(instanceId);
  const max = advice ? Math.max(2048, Math.floor(advice.maxGameMb / STEP) * STEP) : 16384;
  const total = advice?.totalMb ?? 16384;
  const min = 1024;
  const clamped = Math.min(Math.max(value, min), Math.max(max, value));
  const pct = (v: number) => ((Math.min(v, max) - min) / (max - min)) * 100;
  const risky = clamped > total * 0.75;
  const recommended = advice?.recommendedMb ?? null;
  const [text, setText] = useState(gb(clamped));
  useEffect(() => setText(gb(clamped)), [clamped]);

  const commitText = () => {
    const n = parseFloat(text.replace(",", "."));
    if (Number.isFinite(n) && n > 0) onChange(Math.round((n * 1024) / 256) * 256);
    else setText(gb(clamped));
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-3">
        <div className="relative flex-1 py-2">
          <div className="relative h-1.5 rounded-full bg-slate-700">
            {/* The part of the range that leaves the system short. */}
            <div className="absolute inset-y-0 right-0 rounded-r-full bg-redstone/35" style={{ left: `${pct(total * 0.75)}%` }} />
            <motion.div className={`absolute inset-y-0 left-0 rounded-l-full ${risky ? "bg-redstone" : "bg-gold"}`} animate={{ width: `${pct(clamped)}%` }} transition={{ type: "spring", stiffness: 600, damping: 40 }} />
            {recommended && recommended <= max && (
              <button
                type="button"
                onClick={() => onChange(recommended)}
                className="group absolute -top-1.5 h-4.5 w-1.5 -translate-x-1/2 rounded-full bg-ink-2 transition-colors hover:bg-gold"
                style={{ left: `${pct(recommended)}%` }}
                aria-label={`Use the recommended ${gb(recommended)} GB`}
              >
                <span className="pointer-events-none absolute -top-7 left-1/2 -translate-x-1/2 whitespace-nowrap rounded-[5px] border border-line bg-slate-800 px-1.5 py-0.5 text-[11px] text-ink opacity-0 transition-opacity group-hover:opacity-100">
                  Recommended {gb(recommended)} GB
                </span>
              </button>
            )}
          </div>
          <input
            type="range"
            min={min}
            max={max}
            step={STEP}
            value={Math.min(clamped, max)}
            onChange={(e) => onChange(Number(e.target.value))}
            className="memory-range absolute inset-0 w-full cursor-pointer opacity-0"
            aria-label="Memory"
          />
          <motion.span
            className={`pointer-events-none absolute top-1/2 h-4 w-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-slate-950 shadow ${risky ? "bg-redstone" : "bg-gold"}`}
            animate={{ left: `${pct(clamped)}%` }}
            transition={{ type: "spring", stiffness: 600, damping: 40 }}
          />
        </div>
        <label className="flex items-center gap-1.5">
          <input
            className="field h-8 w-16 text-right tabular-nums"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onBlur={commitText}
            onKeyDown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
            aria-label="Memory in GB"
          />
          <span className="text-[13px] text-ink-3">GB</span>
        </label>
      </div>
      <p className={`text-[12px] ${risky ? "text-redstone" : "text-ink-3"}`}>
        {risky
          ? `That leaves less than a quarter of your ${gb(total)} GB for everything else; Windows and the game may start swapping.`
          : `Your PC has ${gb(total)} GB. ${recommended ? `For ${advice?.mods} mods, ${gb(recommended)} GB is plenty; more doesn't make the game faster.` : "Vanilla runs well on 2 to 4 GB; big modpacks want 6 to 8."}`}
      </p>
    </div>
  );
}
