// Mirrors the Rust types in crates/alloy-core. Keep field names in sync (camelCase on the wire).

export type Loader = "vanilla" | "fabric" | "quilt" | "forge" | "neoforge";
export type InstallState = "not_installed" | "installing" | "installed" | "failed";

export interface Instance {
  id: string;
  name: string;
  icon?: string | null;
  group?: string | null;
  gameVersion: string;
  loader: Loader;
  loaderVersion?: string | null;
  javaPath?: string | null;
  memoryMb?: number | null;
  jvmArgs: string[];
  gameArgs: string[];
  resolution?: { width: number; height: number } | null;
  env: [string, string][];
  account?: string | null;
  linkedPack?: {
    source: "modrinth" | "curseforge" | "url" | "file";
    projectId?: string | null;
    versionId?: string | null;
    name?: string | null;
    versionName?: string | null;
  } | null;
  installState: InstallState;
  createdAt: number;
  lastPlayed?: number | null;
  playTimeSecs: number;
}

export interface NewInstance {
  name: string;
  gameVersion: string;
  loader: Loader;
  loaderVersion?: string | null;
  icon?: string | null;
}

export interface GameVersion {
  id: string;
  type: "release" | "snapshot" | "old_beta" | "old_alpha";
  releaseTime: string;
}

export interface LoaderVersion {
  id: string;
  stable: boolean;
}

export interface Account {
  id: string;
  kind: "microsoft" | "offline";
  username: string;
  skinUrl?: string | null;
  skinVariant?: string | null;
  capeUrl?: string | null;
  addedAt: number;
  needsLogin: boolean;
}

export interface Settings {
  theme: "dark" | "light" | "oled" | "system";
  accent: string;
  motion: "full" | "calm" | "off";
  background: "forge" | "plain";
  defaultMemoryMb: number;
  defaultJvmArgs: string[];
  maxConcurrentDownloads: number;
  onLaunch: "keep_open" | "minimize" | "close";
  customMsaClientId?: string | null;
  curseforgeApiKey?: string | null;
}

export type Stage =
  | "metadata" | "java" | "libraries" | "assets" | "loader"
  | "content" | "extracting" | "starting" | "done";

export interface Progress {
  stage: Stage;
  current: number;
  total: number;
  unit: "files" | "bytes" | "steps";
  message?: string | null;
}

export type ContentKind = "mod" | "resourcepack" | "shader" | "datapack" | "modpack";
export type Platform = "modrinth" | "curseforge";

export interface ContentHit {
  source: Platform;
  id: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  iconUrl?: string | null;
  downloads: number;
  follows: number;
  updated?: string | null;
  categories: string[];
  kind: ContentKind;
  gallery: string[];
  color?: number | null;
}

export interface SearchPage {
  hits: ContentHit[];
  total: number;
  offset: number;
}

export interface SearchQuery {
  query: string;
  kind?: ContentKind;
  gameVersion?: string;
  loader?: string;
  sort?: string;
  offset?: number;
  limit?: number;
}

export interface ModrinthVersion {
  id: string;
  project_id: string;
  name: string;
  version_number: string;
  game_versions: string[];
  loaders: string[];
  version_type: string;
  date_published: string;
  downloads: number;
}

export interface InstalledContent {
  path: string;
  fileName: string;
  kind: ContentKind;
  enabled: boolean;
  size: number;
  meta?: {
    source: "modrinth" | "curseforge" | "local";
    projectId?: string | null;
    title?: string | null;
    versionName?: string | null;
    iconUrl?: string | null;
  } | null;
}

export interface GameStateEvent {
  instanceId: string;
  state: "starting" | "running" | "exited";
  exitCode?: number | null;
  error?: string | null;
}

export interface FoundUpdate {
  path: string;
  current: string;
  newer: boolean;
  manifest: { version: string; platform: string; kind: string; notes: string; date: string };
}

export interface UpdateOutcome {
  ok: boolean;
  from: string;
  to: string;
  notes: string;
}
