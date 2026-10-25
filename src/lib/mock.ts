// Browser-only stand-in for the Rust backend, used by `pnpm dev` outside Tauri.
// Search hits the real Modrinth API (it allows CORS); everything else is in memory.

import { emitLocal } from "./bus";
import type { Account, Instance, InstalledContent, Settings, Stage } from "./types";

const now = Math.floor(Date.now() / 1000);
const inst = (p: Partial<Instance> & Pick<Instance, "id" | "name" | "gameVersion" | "loader">): Instance => ({
  jvmArgs: [], gameArgs: [], env: [], installState: "installed", createdAt: now - 86400 * 30,
  playTimeSecs: 0, ...p,
});

const db = {
  instances: [
    inst({ id: "survival-26-2", name: "Survival 26.2", gameVersion: "26.2", loader: "vanilla", lastPlayed: now - 3600 * 5, playTimeSecs: 3600 * 41 }),
    inst({ id: "fabulously-optimized", name: "Fabulously Optimized", gameVersion: "1.21.1", loader: "fabric", loaderVersion: "0.20.1", lastPlayed: now - 86400 * 2, playTimeSecs: 3600 * 12, linkedPack: { source: "modrinth", name: "Fabulously Optimized", versionName: "6.4.0" } }),
    inst({ id: "create-above-beyond", name: "Create: Above and Beyond", gameVersion: "1.20.1", loader: "forge", loaderVersion: "47.4.0", lastPlayed: now - 86400 * 9, playTimeSecs: 3600 * 77, linkedPack: { source: "curseforge", name: "Create: Above and Beyond", versionName: "1.4" } }),
    inst({ id: "skyblock", name: "Skyblock with Ben", gameVersion: "1.21.1", loader: "neoforge", loaderVersion: "21.1.77", lastPlayed: now - 86400 * 20 }),
    inst({ id: "snapshot-testing", name: "Snapshot testing", gameVersion: "26.3-snapshot-5", loader: "vanilla", installState: "not_installed" }),
  ] as Instance[],
  accounts: [
    { id: "a1b2c3", kind: "microsoft", username: "Elchi", addedAt: now, needsLogin: false },
    { id: "d4e5f6", kind: "offline", username: "TestBuilder", addedAt: now, needsLogin: false },
  ] as Account[],
  active: "a1b2c3" as string | null,
  settings: {
    theme: "dark", accent: "#f8c95a", motion: "full", background: "forge", defaultMemoryMb: 4096, defaultJvmArgs: [],
    maxConcurrentDownloads: 16, onLaunch: "keep_open", customMsaClientId: null, curseforgeApiKey: null,
  } as Settings,
  running: new Set<string>(),
};

const content: InstalledContent[] = [
  { path: "mods/sodium.jar", fileName: "sodium-fabric-0.8.13+mc1.21.1.jar", kind: "mod", enabled: true, size: 1_204_000, meta: { source: "modrinth", title: "Sodium", versionName: "0.8.13", iconUrl: null } },
  { path: "mods/lithium.jar", fileName: "lithium-fabric-0.15.0.jar", kind: "mod", enabled: true, size: 760_000, meta: { source: "modrinth", title: "Lithium", versionName: "0.15.0" } },
  { path: "mods/iris.jar", fileName: "iris-fabric-1.9.1.jar", kind: "mod", enabled: false, size: 2_400_000, meta: { source: "modrinth", title: "Iris Shaders", versionName: "1.9.1" } },
  { path: "mods/jei.jar", fileName: "jei-1.21.1-fabric-19.21.jar", kind: "mod", enabled: true, size: 1_900_000, meta: { source: "curseforge", title: "Just Enough Items", versionName: "19.21.0" } },
  { path: "resourcepacks/faithful.zip", fileName: "Faithful 32x.zip", kind: "resourcepack", enabled: true, size: 9_100_000, meta: null },
];

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function fakeJob(jobId: string, instanceId: string | null) {
  const stages: [Stage, number][] = [["metadata", 1], ["java", 140], ["libraries", 70], ["assets", 5057]];
  for (const [stage, total] of stages) {
    for (let i = 0; i <= 10; i++) {
      emitLocal("job-progress", { jobId, instanceId, progress: { stage, current: Math.round((total * i) / 10), total, unit: "files" } });
      await sleep(70);
    }
  }
}

async function modrinthSearch(args: any) {
  const q = args.query;
  const facets: string[][] = [[`project_type:${q.kind ?? "mod"}`]];
  if (q.gameVersion) facets.push([`versions:${q.gameVersion}`]);
  if (q.loader && (q.kind ?? "mod") === "mod") facets.push([`categories:${q.loader}`]);
  const url = new URL("https://api.modrinth.com/v2/search");
  url.searchParams.set("query", q.query ?? "");
  url.searchParams.set("facets", JSON.stringify(facets));
  url.searchParams.set("index", q.sort ?? "relevance");
  url.searchParams.set("limit", String(q.limit ?? 20));
  url.searchParams.set("offset", String(q.offset ?? 0));
  const r = await fetch(url);
  const j = await r.json();
  return {
    total: j.total_hits, offset: j.offset,
    hits: j.hits.map((h: any) => ({
      source: "modrinth", id: h.project_id, slug: h.slug, title: h.title, description: h.description,
      author: h.author, iconUrl: h.icon_url, downloads: h.downloads, follows: h.follows,
      updated: h.date_modified, categories: h.display_categories, kind: h.project_type,
      gallery: h.gallery ?? [], color: h.color,
    })),
  };
}

export async function mockInvoke<T>(cmd: string, args: any = {}): Promise<T> {
  await sleep(40);
  const r = (v: unknown) => v as T;
  switch (cmd) {
    case "app_info": return r({ version: "0.1.0", dataDir: "~/.local/share/alloy", os: "linux", curseforgeBuiltin: false });
    case "get_settings": return r(db.settings);
    case "get_textures": return r({});
    case "check_updates": return r(null);
    case "update_outcome": return r(null);
    case "inspect_update":
    case "apply_update": throw "Updates are applied by the desktop app.";
    case "prewarm_launch": return r(undefined);
    case "save_settings": db.settings = args.settings; return r(undefined);
    case "list_game_versions": {
      const ids = ["26.2", "26.1.2", "26.1.1", "26.1", "1.21.11", "1.21.10", "1.21.8", "1.21.4", "1.21.1", "1.20.6", "1.20.4", "1.20.1", "1.19.4", "1.18.2", "1.16.5", "1.12.2", "1.8.9", "1.7.10"];
      const snaps = args.includeSnapshots ? ["26.3-snapshot-5", "26.3-snapshot-4"] : [];
      return r([...snaps.map((id) => ({ id, type: "snapshot", releaseTime: "" })), ...ids.map((id) => ({ id, type: "release", releaseTime: "" }))]);
    }
    case "list_loader_versions": return r(args.loader === "vanilla" ? [] : [{ id: "0.20.1", stable: true }, { id: "0.20.0", stable: true }, { id: "0.21.0-beta.2", stable: false }]);
    case "list_instances": return r([...db.instances].sort((a, b) => (b.lastPlayed ?? b.createdAt) - (a.lastPlayed ?? a.createdAt)));
    case "get_instance": return r(db.instances.find((i) => i.id === args.id));
    case "create_instance": {
      const n = args.new;
      const created = inst({ id: n.name.toLowerCase().replace(/[^a-z0-9]+/g, "-"), name: n.name, gameVersion: n.gameVersion, loader: n.loader, loaderVersion: n.loaderVersion ?? (n.loader === "vanilla" ? null : "0.20.1"), installState: "installing", createdAt: Math.floor(Date.now() / 1000) });
      db.instances.push(created);
      emitLocal("instances-changed", null);
      fakeJob(`install:${created.id}`, created.id).then(() => { created.installState = "installed"; emitLocal("instances-changed", null); });
      return r(created);
    }
    case "update_instance": db.instances = db.instances.map((i) => (i.id === args.instance.id ? args.instance : i)); emitLocal("instances-changed", null); return r(undefined);
    case "delete_instance": db.instances = db.instances.filter((i) => i.id !== args.id); emitLocal("instances-changed", null); return r(undefined);
    case "duplicate_instance": { const s = db.instances.find((i) => i.id === args.id)!; const c = { ...s, id: s.id + "-copy", name: args.name, lastPlayed: null }; db.instances.push(c); emitLocal("instances-changed", null); return r(c); }
    case "install_instance": await fakeJob(args.jobId, args.id); return r(undefined);
    case "open_instance_folder": return r(undefined);
    case "launch_instance": {
      const id = args.id;
      emitLocal("game-state", { instanceId: id, state: "starting" });
      const i = db.instances.find((x) => x.id === id)!;
      if (i.installState !== "installed") { await fakeJob(`launch:${id}`, id); i.installState = "installed"; }
      await sleep(600);
      db.running.add(id);
      i.lastPlayed = Math.floor(Date.now() / 1000);
      emitLocal("game-state", { instanceId: id, state: "running" });
      emitLocal("instances-changed", null);
      const lines = ["[Render thread/INFO]: Setting user: Elchi", "[Render thread/INFO]: Backend library: LWJGL version 3.4.1", "[Render thread/INFO]: Reloading ResourceManager: vanilla", "[Worker-Main-2/INFO]: Loaded 1407 recipes", "[Render thread/WARN]: Missing sound for event: minecraft:item.goat_horn.play", "[Render thread/INFO]: Sound engine started"];
      lines.forEach((line, n) => setTimeout(() => emitLocal("game-log", { instanceId: id, stream: "stdout", line }), 300 * (n + 1)));
      return r(undefined);
    }
    case "stop_instance": db.running.delete(args.id); emitLocal("game-state", { instanceId: args.id, state: "exited", exitCode: 0 }); return r(undefined);
    case "running_instances": return r([...db.running]);
    case "list_accounts": return r({ accounts: db.accounts, active: db.active });
    case "add_offline_account": { const a: Account = { id: crypto.randomUUID().replace(/-/g, ""), kind: "offline", username: args.name, addedAt: now, needsLogin: false }; db.accounts.push(a); db.active = a.id; emitLocal("accounts-changed", null); return r(a); }
    case "login_microsoft": await sleep(1200); throw "The Microsoft login window opens in the desktop app.";
    case "remove_account": db.accounts = db.accounts.filter((a) => a.id !== args.id); if (db.active === args.id) db.active = db.accounts[0]?.id ?? null; emitLocal("accounts-changed", null); return r(undefined);
    case "set_active_account": db.active = args.id; emitLocal("accounts-changed", null); return r(undefined);
    case "search_content":
      if (args.platform === "curseforge" && !db.settings.curseforgeApiKey) throw "CurseForge support is a work in progress";
      return r(await modrinthSearch(args));
    case "modrinth_versions": {
      const res = await fetch(`https://api.modrinth.com/v2/project/${args.projectId}/version`);
      return r(await res.json());
    }
    case "install_modrinth_version": await sleep(900); emitLocal("content-changed", null); return r(["Installed"]);
    case "install_modrinth_modpack":
    case "import_modpack": await fakeJob(args.jobId, null); throw "Modpack installs run in the desktop app.";
    case "list_content": return r(content);
    case "set_content_enabled": { const c = content.find((x) => x.path === args.path); if (c) c.enabled = args.enabled; emitLocal("content-changed", null); return r(undefined); }
    case "remove_content": { const idx = content.findIndex((x) => x.path === args.path); if (idx >= 0) content.splice(idx, 1); emitLocal("content-changed", null); return r(undefined); }
  }
  throw `mock: unknown command ${cmd}`;
}
