import { create } from "zustand";
import type { FoundUpdate, Job, Platform, Preflight, Progress } from "./types";

export type Page =
  | { name: "home" }
  | { name: "library" }
  | { name: "discover"; target?: string; kind?: "mod" | "modpack" | "resourcepack" | "shader" }
  | { name: "instance"; id: string; tab?: "content" | "logs" | "settings" }
  | { name: "project"; platform: Platform; id: string; target?: string }
  | { name: "settings" };

export interface LogLine {
  stream: "stdout" | "stderr";
  line: string;
  level: "info" | "warn" | "error" | "debug";
}

export interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  text: string;
  action?: { label: string; run: () => void };
}

interface State {
  page: Page;
  history: Page[];
  go: (p: Page) => void;
  back: () => void;

  /** Latest progress per instance (installs and launch preparation). */
  progress: Record<string, Progress | undefined>;
  /** Progress for jobs without an instance yet (modpack imports). */
  jobs: Record<string, Progress | undefined>;
  setProgress: (jobId: string, instanceId: string | null, p: Progress) => void;
  clearProgress: (instanceId: string) => void;

  game: Record<string, "starting" | "running" | undefined>;
  setGame: (id: string, s: "starting" | "running" | undefined) => void;

  logs: Record<string, LogLine[]>;
  pushLog: (id: string, l: LogLine) => void;
  clearLogs: (id: string) => void;

  toasts: Toast[];
  toast: (t: Omit<Toast, "id">) => void;
  dismiss: (id: number) => void;

  createOpen: boolean;
  setCreateOpen: (v: boolean) => void;

  /** Block textures from Mojang's jar as data URLs, keyed by name ("gold_ore"). */
  textures: Record<string, string>;
  setTextures: (t: Record<string, string>) => void;

  /** A verified update waiting to be applied. */
  update: FoundUpdate | null;
  setUpdate: (u: FoundUpdate | null) => void;

  /** Every job the downloads panel shows. */
  jobList: Job[];
  setJobList: (j: Job[]) => void;
  /** Download speed while anything runs, bytes per second. */
  speed: number;
  setSpeed: (n: number) => void;
  /** The downloads panel, opened from its sidebar button or a toast's "Show". */
  downloadsOpen: boolean;
  setDownloadsOpen: (open: boolean) => void;

  /** Problems found right before launching; the dialog shows while this is set. */
  preflight: { instanceId: string; result: Preflight } | null;
  setPreflight: (p: { instanceId: string; result: Preflight } | null) => void;

  /** What the window is being dragged over, for the drop overlay. */
  dropping: boolean;
  setDropping: (v: boolean) => void;

  /** An image dropped on an instance page, waiting for the icon editor. */
  iconDrop: { instanceId: string; src: string } | null;
  setIconDrop: (d: { instanceId: string; src: string } | null) => void;
}

const MAX_LOG = 5000;
let toastId = 0;

export const useStore = create<State>((set, get) => ({
  page: { name: "home" },
  history: [],
  go: (p) => set((s) => ({ page: p, history: [...s.history.slice(-30), s.page] })),
  back: () => {
    const h = get().history;
    if (h.length) set({ page: h[h.length - 1], history: h.slice(0, -1) });
  },

  progress: {},
  jobs: {},
  setProgress: (jobId, instanceId, p) =>
    set((s) => ({
      ...(instanceId
        ? { progress: { ...s.progress, [instanceId]: p.stage === "done" ? undefined : p } }
        : { jobs: { ...s.jobs, [jobId]: p.stage === "done" ? undefined : p } }),
      jobList: s.jobList.some((j) => j.id === jobId) ? s.jobList.map((j) => (j.id === jobId ? { ...j, progress: p } : j)) : s.jobList,
    })),
  clearProgress: (id) => set((s) => ({ progress: { ...s.progress, [id]: undefined } })),

  game: {},
  setGame: (id, v) => set((s) => ({ game: { ...s.game, [id]: v } })),

  logs: {},
  pushLog: (id, l) =>
    set((s) => {
      const cur = s.logs[id] ?? [];
      const next = cur.length >= MAX_LOG ? [...cur.slice(cur.length - MAX_LOG + 1), l] : [...cur, l];
      return { logs: { ...s.logs, [id]: next } };
    }),
  clearLogs: (id) => set((s) => ({ logs: { ...s.logs, [id]: [] } })),

  toasts: [],
  toast: (t) => {
    const id = ++toastId;
    set((s) => ({ toasts: [...s.toasts, { ...t, id }] }));
    setTimeout(() => get().dismiss(id), t.kind === "error" ? 8000 : 4000);
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  createOpen: false,
  setCreateOpen: (v) => set({ createOpen: v }),

  textures: {},
  setTextures: (textures) => set({ textures }),

  update: null,
  setUpdate: (update) => set({ update }),

  jobList: [],
  setJobList: (jobList) => set({ jobList }),
  speed: 0,
  setSpeed: (speed) => set({ speed }),
  downloadsOpen: false,
  setDownloadsOpen: (downloadsOpen) => set({ downloadsOpen }),

  preflight: null,
  setPreflight: (preflight) => set({ preflight }),

  dropping: false,
  setDropping: (dropping) => set({ dropping }),

  iconDrop: null,
  setIconDrop: (iconDrop) => set({ iconDrop }),
}));

export const toastError = (e: unknown) => {
  const text = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  // Cancelling is the user's own choice; the downloads panel already shows it.
  if (text === "Cancelled") return;
  useStore.getState().toast({ kind: "error", text });
};
