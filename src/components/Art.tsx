// Instance icons and player heads.

import { useMemo, useState } from "react";
import { motion } from "motion/react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../lib/api";
import type { Account, Instance } from "../lib/types";
import { hueOf } from "../lib/format";
import { useStore } from "../lib/store";

/** Ores an instance icon can be, from the textures fetched at install time. */
const ORES = [
  "gold_ore", "diamond_ore", "emerald_ore", "lapis_ore", "redstone_ore", "iron_ore", "copper_ore", "coal_ore",
  "deepslate_gold_ore", "deepslate_diamond_ore", "deepslate_emerald_ore", "deepslate_lapis_ore",
  "deepslate_redstone_ore", "deepslate_iron_ore", "deepslate_copper_ore", "deepslate_coal_ore",
  "nether_gold_ore", "nether_quartz_ore", "ancient_debris_side", "raw_gold_block",
];

export function oreFor(seed: string): string {
  let h = 0;
  for (const c of seed) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return ORES[h % ORES.length];
}

/** A real block texture as a tile, with a soft bevel so it reads as a block. */
export function BlockTile({ name, size = 48, className = "" }: { name: string; size?: number; className?: string }) {
  const src = useStore((s) => s.textures[name]);
  if (!src) return <OreBlock seed={name} size={size} />;
  return (
    <span
      className={`relative block shrink-0 overflow-hidden rounded-[6px] ${className}`}
      style={{ width: size, height: size, boxShadow: "inset 0 1px 0 rgb(255 255 255 / 0.18), inset 0 -2px 0 rgb(0 0 0 / 0.35)" }}
    >
      <img src={src} alt="" draggable={false} className="pixelated absolute inset-0 h-full w-full" />
      <span className="pointer-events-none absolute inset-0 rounded-[6px]" style={{ boxShadow: "inset 0 1px 0 rgb(255 255 255 / 0.18), inset 0 -2px 0 rgb(0 0 0 / 0.35)" }} />
    </span>
  );
}

/** Deterministic PRNG so a generated icon never changes between renders. */
function rng(seed: number) {
  let s = seed || 1;
  return () => ((s = (s * 16807) % 2147483647) / 2147483647);
}

/**
 * Instances without an icon get an "ore block": 8x8 stone with specks in a colour
 * derived from the instance id. Recognisable at 24px, never two the same.
 */
export function OreBlock({ seed, size = 48 }: { seed: string; size?: number }) {
  const cells = useMemo(() => {
    const hue = hueOf(seed);
    let h = 0;
    for (const c of seed) h = (h * 33 + c.charCodeAt(0)) >>> 0;
    const r = rng(h % 2147483646);
    const out: { x: number; y: number; fill: string }[] = [];
    for (let y = 0; y < 8; y++)
      for (let x = 0; x < 8; x++) {
        const v = r();
        const stone = 30 + Math.round(r() * 9);
        let fill = `hsl(225 9% ${stone}%)`;
        if (v > 0.8) fill = `hsl(${hue} 70% ${52 + Math.round(r() * 14)}%)`;
        else if (v > 0.72) fill = `hsl(${hue} 45% 34%)`;
        out.push({ x, y, fill });
      }
    return out;
  }, [seed]);
  return (
    <svg width={size} height={size} viewBox="0 0 8 8" shapeRendering="crispEdges" aria-hidden className="shrink-0 rounded-[4px]">
      {cells.map((c) => (
        <rect key={`${c.x}-${c.y}`} x={c.x} y={c.y} width="1" height="1" fill={c.fill} />
      ))}
    </svg>
  );
}

/** An instance's icon: its own image, or its ore. `shared` links it across pages (morph). */
export function InstanceIcon({ instance, size = 48, shared = false, tilt = false }: { instance: Pick<Instance, "id" | "icon">; size?: number; shared?: boolean; tilt?: boolean }) {
  const [broken, setBroken] = useState(false);
  const icon = instance.icon ?? "";
  // A picture the user chose lives in the instance folder; fetched once as a data URL.
  const own = useQuery({ queryKey: ["instanceIcon", instance.id, icon], queryFn: () => api.instanceIcon(instance.id), enabled: icon === "icon.png", staleTime: Infinity });
  const block = icon.startsWith("block:") ? icon.slice(6) : null;
  const src = broken ? undefined : icon.startsWith("http") ? icon : icon === "icon.png" ? (own.data ?? undefined) : undefined;
  const inner = block ? (
    <BlockTile name={block} size={size} />
  ) : src ? (
    <img
      src={src}
      width={size}
      height={size}
      onError={() => setBroken(true)}
      className="shrink-0 rounded-[6px] object-cover"
      style={{ width: size, height: size }}
      alt=""
    />
  ) : (
    <BlockTile name={oreFor(instance.id)} size={size} />
  );
  const wrapped = <span className={`block shrink-0 ${tilt ? "tilt" : ""}`}>{inner}</span>;
  return shared ? (
    <motion.span layoutId={`icon-${instance.id}`} className="block shrink-0" transition={{ type: "spring", stiffness: 380, damping: 34 }}>
      {wrapped}
    </motion.span>
  ) : (
    wrapped
  );
}

/**
 * The face from a 64x64 skin texture (8x8 at 8,8) plus the hat layer (8x8 at 40,8),
 * cropped with background-position so no canvas or CORS is involved.
 */
export function PlayerHead({ account, size = 32 }: { account?: Pick<Account, "username" | "skinUrl"> | null; size?: number }) {
  const scale = size / 8;
  if (!account?.skinUrl) {
    const hue = hueOf(account?.username ?? "?");
    return (
      <div
        className="grid shrink-0 place-items-center rounded-[4px] font-pixel font-bold text-white/90"
        style={{ width: size, height: size, background: `hsl(${hue} 35% 38%)`, fontSize: size * 0.5 }}
        aria-hidden
      >
        {(account?.username ?? "?").slice(0, 1).toUpperCase()}
      </div>
    );
  }
  const layer = (x: number) => ({
    backgroundImage: `url(${account.skinUrl})`,
    backgroundSize: `${64 * scale}px auto`,
    backgroundPosition: `-${x * scale}px -${8 * scale}px`,
  });
  return (
    <div className="pixelated relative shrink-0 overflow-hidden rounded-[4px]" style={{ width: size, height: size }} aria-hidden>
      <div className="absolute inset-0 pixelated" style={layer(8)} />
      <div className="absolute inset-0 pixelated" style={layer(40)} />
    </div>
  );
}
