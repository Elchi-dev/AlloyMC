import type { Loader, Progress } from "./types";

export const loaderName: Record<Loader, string> = {
  vanilla: "Vanilla",
  fabric: "Fabric",
  quilt: "Quilt",
  forge: "Forge",
  neoforge: "NeoForge",
};

export function compact(n: number): string {
  if (n >= 1e9) return (n / 1e9).toFixed(1).replace(/\.0$/, "") + "B";
  if (n >= 1e6) return (n / 1e6).toFixed(1).replace(/\.0$/, "") + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1).replace(/\.0$/, "") + "k";
  return String(n);
}

export function bytes(n: number): string {
  if (n >= 1 << 30) return (n / (1 << 30)).toFixed(1) + " GB";
  if (n >= 1 << 20) return (n / (1 << 20)).toFixed(1) + " MB";
  if (n >= 1 << 10) return Math.round(n / (1 << 10)) + " KB";
  return n + " B";
}

export function playTime(secs: number): string {
  if (secs < 60) return "Not played yet";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return h ? `${h} h ${m} min played` : `${m} min played`;
}

export function ago(unix?: number | null): string {
  if (!unix) return "Never played";
  const s = Math.floor(Date.now() / 1000) - unix;
  if (s < 60) return "Just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  const d = Math.floor(s / 86400);
  if (d === 1) return "Yesterday";
  if (d < 30) return `${d} days ago`;
  return new Date(unix * 1000).toLocaleDateString();
}

const stageLabel: Record<Progress["stage"], string> = {
  metadata: "Reading versions",
  java: "Getting Java",
  libraries: "Downloading libraries",
  assets: "Downloading assets",
  loader: "Installing loader",
  content: "Downloading mods",
  extracting: "Unpacking",
  starting: "Starting",
  done: "Done",
};

export function progressLabel(p: Progress): string {
  return p.message ?? stageLabel[p.stage];
}

export function progressFraction(p: Progress): number {
  return p.total > 0 ? Math.min(1, p.current / p.total) : 0;
}

/** Stable hue from a string, for instances without an icon. */
export function hueOf(s: string): number {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}
