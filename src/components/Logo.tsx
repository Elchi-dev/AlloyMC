// Alloy's mark, traced 1:1 from the original artwork (scripts/trace_logo.py).
// The gold and steel halves are separate groups, so they can move:
//   intro  — the halves slide in from either side and lock, the seam flashes molten (once)
//   hover  — they press together and the seam glows
//   press  — a small squash

import { memo, useEffect, useState } from "react";
import logoSvg from "../assets/logo.svg?raw";

let introPlayed = false;

export const Logo = memo(function Logo({ size = 36, intro = false, className = "" }: { size?: number; intro?: boolean; className?: string }) {
  const [playing, setPlaying] = useState(intro && !introPlayed);
  useEffect(() => {
    if (!playing) return;
    introPlayed = true;
    const t = setTimeout(() => setPlaying(false), 1400);
    return () => clearTimeout(t);
  }, [playing]);

  return (
    <span
      className={`alloy-logo group/logo relative inline-grid place-items-center ${playing ? "alloy-intro" : ""} ${className}`}
      style={{ width: size, height: size }}
      aria-label="Alloy"
      role="img"
    >
      <span className="alloy-heat absolute inset-[18%] rounded-full" aria-hidden />
      <span className="relative block h-full w-full" dangerouslySetInnerHTML={{ __html: logoSvg }} />
    </span>
  );
});
