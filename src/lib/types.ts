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
  /** Fabric and Quilt: start with Fabric API (default yes). */
  fabricApi?: boolean;
}

export interface GameVersion {
  id: string;
  type: "release" | "snapshot" | "old_beta" | "old_alpha";
  releaseTime: string;
}

export interface LoaderVersion {
  id: string;
  stable: boolean;
  /** The build a new instance of this game version gets (Forge's own pick for Forge). */
  recommended: boolean;
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
  /** Files finished and total, alongside byte progress. */
  files?: [number, number] | null;
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
  changelog?: string | null;
  dependencies: { project_id?: string | null; version_id?: string | null; dependency_type: string }[];
  game_versions: string[];
  loaders: string[];
  version_type: string;
  date_published: string;
  downloads: number;
  files: { url: string; filename: string; primary: boolean; size: number }[];
}

export type Source = "modrinth" | "curseforge" | "local";
export type Format = "fabric" | "quilt" | "forge" | "neoforge" | "legacyforge";

export interface ContentMeta {
  source: Source;
  projectId?: string | null;
  versionId?: string | null;
  title?: string | null;
  versionName?: string | null;
  iconUrl?: string | null;
  sha1?: string | null;
  auto?: boolean;
  requires?: string[] | null;
}

/** One file in an instance, as the Content tab shows it. */
export interface ContentItem {
  path: string;
  fileName: string;
  kind: ContentKind;
  enabled: boolean;
  size: number;
  title: string;
  meta?: ContentMeta | null;
  modId?: string | null;
  version?: string | null;
  description?: string | null;
  formats: Format[];
  hasJarIcon: boolean;
  requires: string[];
  requiredBy: string[];
}

export type ProblemKind =
  | { type: "missing"; depId: string; wanted: string }
  | { type: "wrongVersion"; depId: string; depPath?: string | null; wanted: string; found: string }
  | { type: "breaks"; otherPath?: string | null; otherId: string; severe: boolean }
  | { type: "duplicate"; modId: string; otherPath: string }
  | { type: "wrongLoader"; madeFor: Format[] }
  | { type: "wrongGame"; wanted: string }
  | { type: "wrongLoaderVersion"; wanted: string; found: string }
  | { type: "wrongJava"; wanted: string; found: number }
  | { type: "serverOnly" };

export type Problem = ProblemKind & {
  severity: "error" | "warning";
  path: string;
  modName: string;
  message: string;
};

export interface Report {
  problems: Problem[];
  requires: Record<string, string[]>;
  requiredBy: Record<string, string[]>;
  orphans: string[];
  modCount: number;
}

export interface BlockedFile {
  title: string;
  fileName: string;
  pageUrl: string;
  sha1?: string | null;
  size: number;
  modId: number;
  fileId: number;
  folder: string;
  iconUrl?: string | null;
}

export interface Overview {
  items: ContentItem[];
  report: Report;
  blocked: BlockedFile[];
  listedMissing: boolean;
}

export type Fix =
  | { action: "installModrinth"; versionId: string; title: string; iconUrl?: string | null }
  | { action: "installCurseForge"; modId: number; fileId: number; title: string; iconUrl?: string | null }
  | { action: "remove"; path: string; title: string }
  | { action: "disable"; path: string; title: string }
  | { action: "setLoaderVersion"; version: string };

export interface Suggestion {
  problem: number;
  path: string;
  message: string;
  fix: Fix;
  label: string;
}

export interface InstallOutcome {
  installed: string[];
  unresolved: string[];
  blocked: BlockedFile[];
}

export interface ContentUpdate {
  path: string;
  title: string;
  iconUrl?: string | null;
  source: Source;
  projectId: string;
  current?: string | null;
  versionId: string;
  versionName: string;
  fileName: string;
  url: string;
  sha1?: string | null;
  size: number;
  date?: string | null;
  prerelease: boolean;
  kind: ContentKind;
  requires: string[];
}

export interface UpdateResult {
  updated: string[];
  heldBack: [string, string][];
  failed: [string, string][];
}

export interface Preflight {
  errors: Problem[];
  warnings: Problem[];
  system: string[];
}

export interface Job {
  id: string;
  title: string;
  instanceId?: string | null;
  state: "running" | "done" | "failed" | "cancelled";
  progress?: Progress | null;
  error?: string | null;
  startedAt: number;
  finishedAt?: number | null;
  canRetry: boolean;
}

export interface LinkPreview {
  platform: "modrinth" | "curseforge" | "url";
  kind: ContentKind;
  title: string;
  iconUrl?: string | null;
  author?: string | null;
  projectId: string;
  versionId?: string | null;
  url: string;
}

export interface AddedFile {
  name: string;
  kind: "modpack" | "mod" | "resourcepack" | "shader" | "datapack" | "world" | "unknown";
  instanceId?: string | null;
  error?: string | null;
}

export interface MemoryAdvice {
  totalMb: number;
  maxGameMb: number;
  recommendedMb?: number | null;
  mods?: number | null;
}

export interface ModrinthProject {
  id: string;
  slug: string;
  title: string;
  description: string;
  body: string;
  project_type: string;
  categories: string[];
  downloads: number;
  followers: number;
  icon_url?: string | null;
  color?: number | null;
  gallery: { url: string; featured: boolean; title?: string | null }[];
  game_versions: string[];
  loaders: string[];
  source_url?: string | null;
  issues_url?: string | null;
  wiki_url?: string | null;
  discord_url?: string | null;
  updated?: string | null;
  client_side?: string | null;
  server_side?: string | null;
}

export interface Dependency {
  projectId: string;
  title: string;
  iconUrl?: string | null;
  kind: string;
  slug: string;
}

export interface CfMod {
  id: number;
  name: string;
  slug: string;
  summary: string;
  downloadCount: number;
  thumbsUpCount: number;
  classId?: number | null;
  logo?: { url: string; thumbnailUrl?: string | null } | null;
  screenshots: { url: string; thumbnailUrl?: string | null }[];
  authors: { name: string }[];
  categories: { name: string }[];
  dateModified?: string | null;
  links?: { websiteUrl?: string | null } | null;
}

export interface CfFile {
  id: number;
  modId: number;
  displayName: string;
  fileName: string;
  downloadUrl?: string | null;
  fileLength: number;
  gameVersions: string[];
  dependencies: { modId: number; relationType: number }[];
  fileDate?: string | null;
  releaseType: number;
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
