import { useEffect, useRef, useState } from "react";
import { Play, Square } from "lucide-react";
import { api } from "../lib/api";
import { progressFraction, progressLabel } from "../lib/format";
import { toastError, useStore } from "../lib/store";
import type { Instance } from "../lib/types";
import { MoltenFace, Sparks } from "./MoltenFx";

type Size = "hero" | "md" | "sm";

const sizes: Record<Size, { cls: string; icon: number; edge: string }> = {
  hero: { cls: "h-14 min-w-52 px-7 text-[22px]", icon: 20, edge: "6px" },
  md: { cls: "h-11 min-w-36 px-5 text-[17px]", icon: 16, edge: "5px" },
  sm: { cls: "h-8 min-w-20 px-3 text-[14px]", icon: 12, edge: "3px" },
};

const HEAT_DELAY = 900; // ms of hovering before the metal starts to heat
const HEAT_UP = 1300; // ms from cold to white-hot
const COOL_DOWN = 650;
const prewarmed = new Map<string, number>();

/** Heat goes 0 -> 1 while hovered (after a delay) and back to 0 when the pointer leaves. */
function useHeat(enabled: boolean) {
  const [heat, setHeat] = useState(0);
  const hovering = useRef(false);
  const timer = useRef<number>(0);
  const raf = useRef(0);
  const value = useRef(0);

  const animateTo = (target: number, ms: number) => {
    cancelAnimationFrame(raf.current);
    const from = value.current;
    const start = performance.now();
    const step = (now: number) => {
      const k = Math.min(1, (now - start) / ms);
      const eased = target > from ? k * k * (3 - 2 * k) : 1 - (1 - k) * (1 - k);
      value.current = from + (target - from) * eased;
      setHeat(value.current);
      if (k < 1) raf.current = requestAnimationFrame(step);
    };
    raf.current = requestAnimationFrame(step);
  };

  const enter = () => {
    if (!enabled) return;
    hovering.current = true;
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => hovering.current && animateTo(1, HEAT_UP), HEAT_DELAY);
  };
  const leave = () => {
    hovering.current = false;
    clearTimeout(timer.current);
    if (value.current > 0) animateTo(0, COOL_DOWN);
  };
  useEffect(() => () => (clearTimeout(timer.current), cancelAnimationFrame(raf.current)), []);
  useEffect(() => {
    if (!enabled) leave();
  }, [enabled]);
  return { heat, enter, leave };
}

/**
 * Play / progress / stop in one control. While preparing, the label becomes the current
 * stage and a fill sweeps across the block, so the button never jumps around the layout.
 * Rest on it and the gold heats up into molten metal (the forge moment).
 */
export function PlayButton({ instance, size = "md" }: { instance: Instance; size?: Size }) {
  const game = useStore((s) => s.game[instance.id]);
  const progress = useStore((s) => s.progress[instance.id]);
  const motionOff = document.documentElement.dataset.motion === "off";
  const s = sizes[size];
  const installing = instance.installState === "installing" || !!progress;
  const busy = game === "starting" || (installing && game !== "running");
  const idle = !busy && game !== "running";
  const { heat, enter, leave } = useHeat(idle && size !== "sm" && !motionOff);
  const [burst, setBurst] = useState(0);
  const [afterglow, setAfterglow] = useState(false);
  const hot = heat > 0.02;
  // keep the spark canvas alive briefly after a burst, then unmount it (no idle rAF loop)
  useEffect(() => {
    if (!burst) return;
    setAfterglow(true);
    const t = setTimeout(() => setAfterglow(false), 1600);
    return () => clearTimeout(t);
  }, [burst]);

  const onEnter = () => {
    enter();
    // Get the token and version files ready while the pointer rests here.
    const last = prewarmed.get(instance.id) ?? 0;
    if (idle && Date.now() - last > 60_000) {
      prewarmed.set(instance.id, Date.now());
      api.prewarm(instance.id).catch(() => {});
    }
  };

  const onClick = async () => {
    if (heat > 0.5) setBurst((b) => b + 1);
    try {
      if (game === "running") await api.stop(instance.id);
      else if (!busy) await api.launch(instance.id);
    } catch (e) {
      toastError(e);
    }
  };

  let label = "Play";
  if (game === "running") label = "Stop";
  else if (progress) label = size === "sm" ? `${Math.round(progressFraction(progress) * 100)}%` : progressLabel(progress);
  else if (game === "starting") label = "Starting";
  else if (installing) label = "Installing";

  return (
    <span className="relative inline-flex">
      <button
        type="button"
        onClick={onClick}
        onPointerEnter={onEnter}
        onPointerLeave={leave}
        disabled={busy}
        data-state={game === "running" ? "running" : busy ? "busy" : "idle"}
        data-heat={hot ? "on" : undefined}
        className={`play-block no-drag overflow-hidden ${size === "sm" ? "play-block-quiet" : ""} ${s.cls}`}
        style={{ ["--edge" as string]: s.edge, ["--heat" as string]: heat.toFixed(3) }}
        aria-label={`${label} ${instance.name}`}
      >
        {hot && <MoltenFace heat={heat} />}
        {busy && (
          <span
            className="play-progress absolute inset-y-0 left-0 transition-[width] duration-200"
            style={{ width: `${progress ? Math.max(4, progressFraction(progress) * 100) : 100}%` }}
            aria-hidden
          />
        )}
        <span
          className="relative z-[2] flex items-center gap-2.5 transition-colors duration-150"
          style={heat > 0.55 ? { color: "#fff8e8", textShadow: "0 1px 0 rgb(80 20 0 / 0.7), 0 0 12px rgb(255 120 30 / 0.8)" } : undefined}
        >
          {game === "running" ? (
            <>
              <span className="running-dot h-2 w-2 rounded-full bg-grass" aria-hidden />
              <Square size={s.icon} fill="currentColor" strokeWidth={0} />
            </>
          ) : (
            !busy && <Play size={s.icon} fill="currentColor" strokeWidth={0} />
          )}
          <span className={busy && size !== "sm" ? "font-sans text-[0.62em] font-semibold" : ""}>{label}</span>
        </span>
      </button>
      {(hot || afterglow) && <Sparks heat={heat} burst={burst} />}
    </span>
  );
}
