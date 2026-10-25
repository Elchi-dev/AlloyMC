// The Alloy forge behind everything. Sharp and alive on Home, blurred and quiet elsewhere.
//
// Layers (back to front): the 4K render of the artwork, its molten parts as SVG that glow
// and flicker, drifting smoke, rising embers, then a shade so content stays readable.
// Everything is transform/opacity only, and pauses while a game runs (see useAmbientPause).

import { memo, useEffect, useMemo, useRef, useState } from "react";
import forgeUrl from "../assets/forge.webp";
import forgeBlurUrl from "../assets/forge-blur.webp";
import overlayUrl from "../assets/forge-overlay.svg?url";
import { useSettings } from "../lib/queries";
import { useStore } from "../lib/store";

const RATIO = 1024 / 434;

// Where the hot things are, in percent of the artwork.
const SOURCES = [
  { x: 23.5, y: 52, spread: 3.5, count: 14 }, // crucible pour
  { x: 52.6, y: 74, spread: 2.2, count: 9 }, // seam pour at the monument's foot
  { x: 80.4, y: 70, spread: 1.4, count: 6 }, // brazier
];

function rng(seed: number) {
  let s = seed;
  return () => ((s = (s * 16807) % 2147483647) / 2147483647);
}

export const Backdrop = memo(function Backdrop() {
  const page = useStore((s) => s.page.name);
  const { data: settings } = useSettings();
  const stage = useRef<HTMLDivElement>(null);
  const home = page === "home";
  // The molten overlay is fetched after first paint so it never delays the window opening.
  const [overlay, setOverlay] = useState("");
  useEffect(() => {
    fetch(overlayUrl).then((r) => r.text()).then(setOverlay).catch(() => {});
  }, []);

  // Mouse parallax: a few pixels, eased, via a CSS variable (no React renders).
  useEffect(() => {
    let tx = 0, ty = 0, x = 0, y = 0, raf = 0;
    const onMove = (e: PointerEvent) => {
      tx = (e.clientX / window.innerWidth - 0.5) * -14;
      ty = (e.clientY / window.innerHeight - 0.5) * -8;
      if (!raf) raf = requestAnimationFrame(step);
    };
    const step = () => {
      x += (tx - x) * 0.08;
      y += (ty - y) * 0.08;
      stage.current?.style.setProperty("--px", `${x.toFixed(2)}px`);
      stage.current?.style.setProperty("--py", `${y.toFixed(2)}px`);
      raf = Math.abs(tx - x) + Math.abs(ty - y) > 0.05 ? requestAnimationFrame(step) : 0;
    };
    window.addEventListener("pointermove", onMove, { passive: true });
    return () => {
      window.removeEventListener("pointermove", onMove);
      cancelAnimationFrame(raf);
    };
  }, []);

  const embers = useMemo(() => {
    const r = rng(42);
    return SOURCES.flatMap((s, si) =>
      Array.from({ length: s.count }, (_, i) => ({
        key: `${si}-${i}`,
        left: s.x + (r() - 0.5) * 2 * s.spread,
        top: s.y + (r() - 0.5) * 4,
        size: 1.5 + r() * 2.5,
        delay: -r() * 8,
        dur: 4.5 + r() * 4,
        drift: (r() - 0.5) * 60,
      })),
    );
  }, []);

  if (settings?.background === "plain") return <div className="fixed inset-0 -z-10 bg-slate-950" aria-hidden />;

  return (
    <div className="pointer-events-none fixed inset-0 -z-10 overflow-hidden bg-[#0b0c0f]" aria-hidden>
      <div
        ref={stage}
        className="backdrop-stage absolute left-1/2 top-1/2"
        style={{
          width: `max(100vw, calc(100vh * ${RATIO}))`,
          height: `max(100vh, calc(100vw / ${RATIO}))`,
          transform: "translate(calc(-50% + var(--px, 0px)), calc(-50% + var(--py, 0px)))",
        }}
      >
        <div className={`absolute inset-0 transition-opacity duration-700 ${home ? "opacity-100" : "opacity-0"}`}>
          <div className="kenburns absolute inset-0">
            <img src={forgeUrl} alt="" className="absolute inset-0 h-full w-full object-cover" draggable={false} />
            <div className={`forge-overlay absolute inset-0 transition-opacity duration-1000 ${overlay ? "opacity-100" : "opacity-0"}`} dangerouslySetInnerHTML={{ __html: overlay }} />
            <div className="smoke smoke-a" />
            <div className="smoke smoke-b" />
            {embers.map((e) => (
              <span
                key={e.key}
                className="bg-ember"
                style={{
                  left: `${e.left}%`,
                  top: `${e.top}%`,
                  width: e.size,
                  height: e.size,
                  animationDelay: `${e.delay}s`,
                  animationDuration: `${e.dur}s`,
                  ["--drift" as string]: `${e.drift}px`,
                }}
              />
            ))}
          </div>
        </div>
        <img
          src={forgeBlurUrl}
          alt=""
          className={`absolute inset-0 h-full w-full scale-105 object-cover transition-opacity duration-700 ${home ? "opacity-0" : "opacity-60"}`}
          draggable={false}
        />
      </div>
      {/* Shade: keeps text readable, darker away from Home. */}
      <div
        className="absolute inset-0 transition-opacity duration-700"
        style={{
          background: home
            ? "linear-gradient(180deg, rgb(11 12 15 / 0.55) 0%, rgb(11 12 15 / 0.15) 40%, rgb(11 12 15 / 0.7) 100%)"
            : "linear-gradient(180deg, rgb(11 12 15 / 0.82), rgb(11 12 15 / 0.9))",
        }}
      />
    </div>
  );
});
