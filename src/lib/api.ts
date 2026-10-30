// Typed wrappers around the Rust commands. In a plain browser (vite dev without Tauri)
// calls go to an in-memory mock so the UI can be designed without the backend.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type {
  Account, AddedFile, CfFile, CfMod, ContentHit, ContentUpdate, Dependency, Fix, GameVersion, InstallOutcome,
  Instance, Job, LinkPreview, Loader, LoaderVersion, MemoryAdvice, ModrinthProject, ModrinthVersion, NewInstance,
  Overview, Platform, Preflight, SearchPage, SearchQuery, Settings, Suggestion, UpdateResult, FoundUpdate,
  UpdateOutcome, BlockedFile,
} from "./types";

type ImportResult = { instance: Instance; blocked: BlockedFile[] };

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return tauriInvoke<T>(cmd, args);
  const { mockInvoke } = await import("./mock");
  return mockInvoke<T>(cmd, args);
}

export const newJobId = () => crypto.randomUUID();

export const api = {
  checkUpdates: () => call<FoundUpdate | null>("check_updates"),
  inspectUpdate: (path: string) => call<FoundUpdate>("inspect_update", { path }),
  updateOutcome: () => call<UpdateOutcome | null>("update_outcome"),
  applyUpdate: (path: string, allowDowngrade = false) => call<void>("apply_update", { path, allowDowngrade }),
  appInfo: () => call<{ version: string; dataDir: string; os: string; curseforgeBuiltin: boolean }>("app_info"),
  getSettings: () => call<Settings>("get_settings"),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),

  gameVersions: (includeSnapshots: boolean) => call<GameVersion[]>("list_game_versions", { includeSnapshots }),
  loaderVersions: (loader: Loader, gameVersion: string) =>
    call<LoaderVersion[]>("list_loader_versions", { loader, gameVersion }),

  instances: () => call<Instance[]>("list_instances"),
  instance: (id: string) => call<Instance>("get_instance", { id }),
  createInstance: (n: NewInstance) => call<Instance>("create_instance", { new: n }),
  updateInstance: (instance: Instance) => call<void>("update_instance", { instance }),
  deleteInstance: (id: string) => call<void>("delete_instance", { id }),
  duplicateInstance: (id: string, name: string) => call<Instance>("duplicate_instance", { id, name }),
  installInstance: (id: string, jobId: string, repair = false) => call<void>("install_instance", { id, jobId, repair }),
  prewarm: (id: string) => call<void>("prewarm_launch", { id }),
  textures: () => call<Record<string, string>>("get_textures"),
  openFolder: (id: string, sub?: string) => call<void>("open_instance_folder", { id, sub }),

  launch: (id: string, quickPlay?: { kind: "world" | "server"; target: string }) =>
    call<void>("launch_instance", { id, quickPlay }),
  stop: (id: string) => call<void>("stop_instance", { id }),
  running: () => call<string[]>("running_instances"),

  accounts: () => call<{ accounts: Account[]; active: string | null }>("list_accounts"),
  addOffline: (name: string) => call<Account>("add_offline_account", { name }),
  loginMicrosoft: () => call<Account>("login_microsoft"),
  removeAccount: (id: string) => call<void>("remove_account", { id }),
  setActiveAccount: (id: string) => call<void>("set_active_account", { id }),

  search: (platform: Platform, query: SearchQuery) => call<SearchPage>("search_content", { platform, query }),
  modrinthProject: (id: string) => call<ModrinthProject>("modrinth_project", { id }),
  modrinthVersions: (projectId: string, instanceId?: string) =>
    call<ModrinthVersion[]>("modrinth_versions", { projectId, instanceId }),
  modrinthDependencies: (versionId: string) => call<Dependency[]>("modrinth_dependencies", { versionId }),
  curseforgeProject: (id: number) => call<CfMod>("curseforge_project", { id }),
  curseforgeDescription: (id: number) => call<string>("curseforge_description", { id }),
  curseforgeFiles: (modId: number, instanceId?: string) => call<CfFile[]>("curseforge_files", { modId, instanceId }),

  installModrinthVersion: (instanceId: string, versionId: string, title?: string) =>
    call<InstallOutcome>("install_modrinth_version", { instanceId, versionId, title, jobId: newJobId() }),
  installCurseForgeFile: (instanceId: string, modId: number, fileId: number, title?: string) =>
    call<InstallOutcome>("install_curseforge_file", { instanceId, modId, fileId, title, jobId: newJobId() }),
  installModrinthModpack: (versionId: string, jobId: string, title?: string) =>
    call<ImportResult>("install_modrinth_modpack", { versionId, jobId, title }),
  installCurseForgeModpack: (modId: number, fileId: number, jobId: string, title?: string) =>
    call<ImportResult>("install_curseforge_modpack", { modId, fileId, jobId, title }),
  importModpack: (source: string, jobId: string) => call<ImportResult>("import_modpack", { source, jobId }),

  overview: (instanceId: string) => call<Overview>("content_overview", { instanceId }),
  contentIcon: (instanceId: string, path: string) => call<string | null>("content_icon", { instanceId, path }),
  setContentEnabled: (instanceId: string, paths: string[], enabled: boolean) =>
    call<void>("set_content_enabled", { instanceId, paths, enabled }),
  removeContent: (instanceId: string, paths: string[]) => call<void>("remove_content", { instanceId, paths }),
  fixes: (instanceId: string) => call<Suggestion[]>("content_fixes", { instanceId }),
  applyFix: (instanceId: string, fix: Fix) => call<InstallOutcome>("apply_fix", { instanceId, fix }),
  checkContentUpdates: (instanceId: string) => call<ContentUpdate[]>("check_content_updates", { instanceId }),
  applyContentUpdates: (instanceId: string, updates: ContentUpdate[]) =>
    call<UpdateResult>("apply_content_updates", { instanceId, updates, jobId: newJobId() }),
  preflight: (instanceId: string) => call<Preflight>("preflight", { instanceId }),
  cleanupOrphans: (instanceId: string) => call<string[]>("cleanup_orphans", { instanceId }),
  fetchListedFiles: (instanceId: string) => call<{ installed: string[]; blocked: BlockedFile[] }>("fetch_listed_files", { instanceId }),
  blockedFiles: (instanceId: string) => call<BlockedFile[]>("blocked_files", { instanceId }),
  dismissBlocked: (instanceId: string, fileId: number) => call<void>("dismiss_blocked", { instanceId, fileId }),
  resolveLink: (url: string) => call<LinkPreview>("resolve_link", { url }),
  addFiles: (instanceId: string | null, paths: string[]) => call<AddedFile[]>("add_files", { instanceId, paths }),

  setInstanceIcon: (id: string, icon: { kind: "image"; data: string } | { kind: "block"; name: string } | { kind: "none" }) =>
    call<void>("set_instance_icon", { id, icon }),
  instanceIcon: (id: string) => call<string | null>("instance_icon", { id }),
  readImageFile: (path: string) => call<string>("read_image_file", { path }),
  memoryAdvice: (instanceId?: string) => call<MemoryAdvice>("memory_advice", { instanceId }),

  jobs: () => call<Job[]>("list_jobs"),
  cancelJob: (id: string) => call<boolean>("cancel_job", { id }),
  retryJob: (id: string) => call<void>("retry_job", { id }),
  clearJobs: () => call<void>("clear_jobs"),
};

export type { ContentHit };
