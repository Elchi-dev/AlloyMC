// Typed wrappers around the Rust commands. In a plain browser (vite dev without Tauri)
// calls go to an in-memory mock so the UI can be designed without the backend.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { useStore } from "./store";
import type {
  AlloyProfile,
  LinkedMinecraft,
  Account, AddedFile, CfFile, CfMod, ContentHit, ContentUpdate, Dependency, Fix, GameVersion, InstallOutcome,
  Instance, Job, LinkPreview, Loader, LoaderVersion, MemoryAdvice, ModrinthProject, ModrinthVersion, NewInstance,
  Overview, Platform, Preflight, SearchPage, SearchQuery, Settings, Suggestion, UpdateResult, FoundUpdate,
  UpdateOutcome, BlockedFile, BisectFound, CrashFix, Diagnosis, SavePoint, SavePoints, ExportCandidate, ExportOptions, ExportReport, FoundInstance } from "./types";

import type { AssistantStatus, Block, Chat, ChatSummary, Mode, Msg } from "./assistant";

type ImportResult = { instance: Instance; blocked: BlockedFile[] };

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  let out: T;
  if (isTauri) out = await tauriInvoke<T>(cmd, args);
  else {
    const { mockInvoke } = await import("./mock");
    out = await mockInvoke<T>(cmd, args);
  }
  noticeBlocked(out, args);
  return out;
}

/** Any install that ends with files only CurseForge's website may hand out asks, once,
 *  whether to fetch them in Alloy's CurseForge window. */
function noticeBlocked(out: unknown, args?: Record<string, unknown>) {
  const o = out as { blocked?: unknown[]; instance?: { id?: string } } | null;
  if (!o || !Array.isArray(o.blocked) || o.blocked.length === 0) return;
  const id = o.instance?.id ?? (typeof args?.instanceId === "string" ? args.instanceId : undefined);
  if (id) useStore.getState().setCurseForgePrompt({ instanceId: id, count: o.blocked.length });
}

export const newJobId = () => crypto.randomUUID();

export const api = {
  updateCheckNow: () => call<FoundUpdate | null>("update_check_now"),
  updatePoke: () => call<void>("update_poke"),
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
  /** The Fabric API build a new Fabric/Quilt instance of this version would get. */
  fabricApiVersion: (loader: Loader, gameVersion: string) => call<string | null>("fabric_api_version", { loader, gameVersion }),

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
  alloymcProfile: (refresh = false) => call<AlloyProfile | null>("alloymc_profile", { refresh }),
  alloymcSignIn: () => call<AlloyProfile>("alloymc_sign_in"),
  alloymcCancelSignIn: () => call<void>("alloymc_cancel_sign_in"),
  alloymcSignOut: () => call<void>("alloymc_sign_out"),
  alloymcLink: (accountId: string) => call<LinkedMinecraft>("alloymc_link_minecraft", { accountId }),
  alloymcUnlink: (uuid: string) => call<void>("alloymc_unlink_minecraft", { uuid }),
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
  crashLast: (instanceId: string) => call<Diagnosis | null>("crash_last", { instanceId }),
  crashDismiss: (instanceId: string) => call<void>("crash_dismiss", { instanceId }),
  crashApplyFix: (instanceId: string, fix: CrashFix) => call<void>("crash_apply_fix", { instanceId, fix }),
  bisectStart: (instanceId: string) => call<BisectFound>("bisect_start", { instanceId }),
  savepoints: (instanceId: string) => call<SavePoints>("savepoints", { instanceId }),
  savepointCreate: (instanceId: string, label: string) => call<SavePoint>("savepoint_create", { instanceId, label }),
  savepointRestore: (instanceId: string, pointId: string) => call<Instance>("savepoint_restore", { instanceId, pointId }),
  savepointRename: (instanceId: string, pointId: string, label: string) => call<void>("savepoint_rename", { instanceId, pointId, label }),
  savepointDelete: (instanceId: string, pointId: string) => call<void>("savepoint_delete", { instanceId, pointId }),
  exportCandidates: (instanceId: string) => call<ExportCandidate[]>("export_candidates", { instanceId }),
  exportInstance: (instanceId: string, options: ExportOptions) => call<ExportReport>("export_instance", { instanceId, options }),
  packSource: (instanceId: string) => call<Instance["linkedPack"] | null>("pack_source", { instanceId }),
  curseforgeWindow: (instanceId: string) => call<void>("curseforge_window", { instanceId }),
  importDiscover: () => call<FoundInstance[]>("import_discover"),
  importInspect: (path: string) => call<FoundInstance[]>("import_inspect", { path }),
  importInstance: (found: FoundInstance) => call<Instance>("import_instance", { found }),
  retryJob: (id: string) => call<void>("retry_job", { id }),
  testLaunch: (instanceId: string) => call<{ started: boolean; diagnosis?: Diagnosis | null }>("test_launch", { instanceId }),
  assistantStatus: () => call<AssistantStatus>("assistant_status"),
  assistantSend: (requestId: string, mode: Mode, messages: Msg[]) =>
    call<{ content: Block[]; stopReason?: string | null }>("assistant_send", { requestId, mode, messages }),
  assistantCancel: (requestId: string) => call<void>("assistant_cancel", { requestId }),
  assistantReadFile: (instanceId: string, path: string) => call<string>("assistant_read_file", { instanceId, path }),
  assistantCurrentText: (instanceId: string, path: string) => call<string | null>("assistant_current_text", { instanceId, path }),
  assistantWriteFile: (instanceId: string, path: string, content: string) => call<void>("assistant_write_file", { instanceId, path, content }),
  assistantReadLog: (instanceId: string, lines?: number, contains?: string) => call<string>("assistant_read_log", { instanceId, lines, contains }),
  assistantCheckpoint: (instanceId: string) => call<void>("assistant_checkpoint", { instanceId }),
  assistantChats: () => call<ChatSummary[]>("assistant_chats"),
  assistantChat: (id: string) => call<Chat>("assistant_chat", { id }),
  assistantChatSave: (chat: Chat) => call<void>("assistant_chat_save", { chat }),
  assistantChatDelete: (id: string) => call<void>("assistant_chat_delete", { id }),
  clearJobs: () => call<void>("clear_jobs"),
};

export type { ContentHit };
