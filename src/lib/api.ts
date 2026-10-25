// Typed wrappers around the Rust commands. In a plain browser (vite dev without Tauri)
// calls go to an in-memory mock so the UI can be designed without the backend.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type {
  Account, ContentHit, GameVersion, Instance, InstalledContent, LoaderVersion,
  ModrinthVersion, NewInstance, Platform, SearchPage, SearchQuery, Settings, Loader,
  FoundUpdate, UpdateOutcome,
} from "./types";

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
  modrinthVersions: (projectId: string, instanceId?: string) =>
    call<ModrinthVersion[]>("modrinth_versions", { projectId, instanceId }),
  installModrinthVersion: (instanceId: string, versionId: string) =>
    call<string[]>("install_modrinth_version", { instanceId, versionId }),
  installModrinthModpack: (versionId: string, jobId: string) =>
    call<{ instance: Instance }>("install_modrinth_modpack", { versionId, jobId }),
  importModpack: (source: string, jobId: string) =>
    call<{ instance: Instance; blocked: { name: string; url: string }[] }>("import_modpack", { source, jobId }),

  content: (instanceId: string) => call<InstalledContent[]>("list_content", { instanceId }),
  setContentEnabled: (instanceId: string, path: string, enabled: boolean) =>
    call<void>("set_content_enabled", { instanceId, path, enabled }),
  removeContent: (instanceId: string, path: string) => call<void>("remove_content", { instanceId, path }),
};

export type { ContentHit };
