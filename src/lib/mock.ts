// Browser-only stand-in for the Rust backend, used by `pnpm dev` outside Tauri.
// Search hits the real Modrinth API (it allows CORS); everything else is in memory.

import { emitLocal } from "./bus";
import type { Account, ContentItem, Instance, Settings, Stage, Diagnosis, SavePoint } from "./types";

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
    maxConcurrentDownloads: 16, onLaunch: "keep_open", customMsaClientId: null, curseforgeApiKey: null, autoUpdate: true,
  } as Settings,
  running: new Set<string>(),
  alloymc: null as null | { handle: string; displayName: string; avatarUrl: string; profileUrl: string; accent: string; followers: number; following: number; email: string; emailVerified: boolean; minecraft: { uuid: string; name: string; shown: boolean }[] },
};

const item = (p: Partial<ContentItem> & Pick<ContentItem, "path" | "title">): ContentItem => ({
  fileName: p.path.split("/")[1], kind: "mod", enabled: true, size: 1_000_000, formats: ["fabric"], hasJarIcon: false, requires: [], requiredBy: [], ...p,
});
const content: ContentItem[] = [
  item({ path: "mods/sodium.jar", title: "Sodium", size: 1_204_000, meta: { source: "modrinth", versionName: "0.8.13" }, requiredBy: ["mods/iris.jar"] }),
  item({ path: "mods/lithium.jar", title: "Lithium", size: 760_000, meta: { source: "modrinth", versionName: "0.15.0" } }),
  item({ path: "mods/iris.jar", title: "Iris Shaders", enabled: false, size: 2_400_000, meta: { source: "modrinth", versionName: "1.9.1" }, requires: ["mods/sodium.jar"] }),
  item({ path: "mods/jei.jar", title: "Just Enough Items", size: 1_900_000, meta: { source: "curseforge", versionName: "19.21.0" } }),
  item({ path: "resourcepacks/faithful.zip", title: "Faithful 32x", kind: "resourcepack", size: 9_100_000, formats: [] }),
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

// A crash to look at in the browser: run alloyMock.crash() in the console.
const demoCrash = (): Diagnosis => ({
  kind: "mixinFailed",
  title: "Iris Shaders doesn't fit this setup",
  detail: "Iris Shaders changes Minecraft's code, and that change failed. Usually it's built for another Minecraft or loader version, or it clashes with another mod that changes the same code. Update it, or turn it off.",
  suspects: [
    { path: "mods/iris.jar", name: "Iris Shaders", modId: "iris", confidence: "high", reason: "A mixin of this mod failed to apply." },
    { path: "mods/sodium.jar", name: "Sodium", modId: "sodium", confidence: "low", reason: "Its code is at the top of the crash." },
  ],
  fixes: [{ kind: "disableMod", path: "mods/iris.jar" }],
  evidence: [
    "Mixin apply for mod iris failed iris.mixins.json:MixinLevelRenderer from mod iris -> net.minecraft.class_761: InjectionError",
    "org.spongepowered.asm.mixin.injection.throwables.InjectionError: Critical injection failure: @Inject annotation on onRender could not find any targets matching 'renderLevel'",
    "at org.spongepowered.asm.mixin.injection.struct.InjectionInfo.validateTargets(InjectionInfo.java:656)",
  ],
  reportPath: "crash-reports/crash-2026-11-01_10.42.13-client.txt",
  afterStart: false,
  canBisect: true,
  exitCode: -1,
  at: Math.floor(Date.now() / 1000),
});
const crashes: Record<string, Diagnosis> = {};
const points: Record<string, SavePoint[]> = {
  "fabulously-optimized": [
    { id: "p2", label: "Before updating 6 files", auto: true, createdAt: now - 86400 * 2, gameVersion: "1.21.1", loader: "fabric", loaderVersion: "0.20.1", mods: 4, size: 6_300_000 },
    { id: "p1", label: "Working setup with Iris", auto: false, createdAt: now - 86400 * 9, gameVersion: "1.21.1", loader: "fabric", loaderVersion: "0.16.5", mods: 3, size: 5_100_000 },
  ],
};
(globalThis as any).alloyMock = {
  crash(id = "fabulously-optimized") {
    crashes[id] = demoCrash();
    emitLocal("crash-diagnosis", { instanceId: id, diagnosis: crashes[id] });
  },
};

let chats: any[] = [];
let big: ContentItem[] | null = null;
/** A 220-mod pack, for long lists. */
const bigPack = () =>
  (big ??= Array.from({ length: 220 }, (_, n) => item({ path: `mods/mod-${String(n).padStart(3, "0")}.jar`, title: `Mod number ${n}`, meta: { source: "curseforge", versionName: `1.${n}` } })));

/** A scripted Claude for designing the panel: looks at instances, proposes a config change, sums up. */
async function mockAssistant(requestId: string, messages: any[]) {
  const last = messages[messages.length - 1];
  const results = last.content.filter((b: any) => b.type === "tool_result");
  const stream = async (index: number, text: string) => {
    for (const word of text.split(/(?<= )/)) {
      emitLocal("assistant-delta", { requestId, delta: { type: "text", index, text: word } });
      await sleep(25);
    }
  };
  const tool = (index: number, name: string, input: unknown) => {
    const id = `toolu_${Math.random().toString(36).slice(2, 10)}`;
    emitLocal("assistant-delta", { requestId, delta: { type: "tool", index, id, name } });
    return { type: "tool_use", id, name, input };
  };
  await sleep(500);
  if (results.length === 0) {
    const text = "Let me look at your instances first.";
    await stream(0, text);
    const t = tool(1, "list_instances", {});
    await sleep(300);
    return { content: [{ type: "text", text }, t], stopReason: "tool_use" };
  }
  if (results.length === 1 && messages.filter((m: any) => m.role === "assistant").length === 1) {
    const text = "**Fabulously Optimized** already has Sodium. Turning leaves to *fast* gives a few more frames in forests:";
    await stream(0, text);
    const t = tool(1, "write_file", {
      instance_id: "fabulously-optimized",
      path: "config/sodium-options.json",
      content: '{\n  "quality": {\n    "weather_quality": "DEFAULT",\n    "leaves_quality": "FAST"\n  },\n  "advanced": {}\n}',
    });
    return { content: [{ type: "text", text }, t], stopReason: "tool_use" };
  }
  const declined = results.some((r: any) => r.is_error);
  const text = declined
    ? "No problem, I left it as it was. Want me to look at something else, like memory or shaders?"
    : "Done. Leaves now render fast in **Fabulously Optimized**.\n\n- It shows the next time you start the game\n- You can switch it back in *Video Settings → Quality*";
  await stream(0, text);
  return { content: [{ type: "text", text }], stopReason: "end_turn" };
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
    case "list_loader_versions": return r(args.loader === "vanilla" ? [] : [{ id: "0.20.1", stable: true, recommended: true }, { id: "0.20.0", stable: true, recommended: false }, { id: "0.21.0-beta.2", stable: false, recommended: false }]);
    case "fabric_api_version": return r(args.loader === "fabric" || args.loader === "quilt" ? `0.116.17+${args.gameVersion}` : null);
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
      lines.forEach((line, n) => setTimeout(() => emitLocal("game-log", { instanceId: id, lines: [{ stream: "stdout", line }] }), 300 * (n + 1)));
      return r(undefined);
    }
    case "stop_instance": db.running.delete(args.id); emitLocal("game-state", { instanceId: args.id, state: "exited", exitCode: 0 }); return r(undefined);
    case "running_instances": return r([...db.running]);
    case "update_check_now": await sleep(800); return r(null);
    case "crash_last": return r(crashes[args.instanceId] ?? null);
    case "savepoints": return r({ points: points[args.instanceId] ?? [], diskBytes: (points[args.instanceId] ?? []).length ? 7_200_000 : 0 });
    case "savepoint_create": {
      const p = { id: `p${Date.now()}`, label: args.label, auto: false, createdAt: Math.floor(Date.now() / 1000), gameVersion: "1.21.1", loader: "fabric" as const, loaderVersion: "0.20.1", mods: content.filter((c) => c.kind === "mod").length, size: 6_300_000 };
      points[args.instanceId] = [p, ...(points[args.instanceId] ?? [])]; emitLocal("savepoints-changed", null); return r(p);
    }
    case "savepoint_restore": await sleep(700); emitLocal("savepoints-changed", null); return r(db.instances.find((i) => i.id === args.instanceId));
    case "savepoint_rename": { const p = points[args.instanceId]?.find((x) => x.id === args.pointId); if (p) { p.label = args.label; p.auto = false; } emitLocal("savepoints-changed", null); return r(undefined); }
    case "savepoint_delete": points[args.instanceId] = (points[args.instanceId] ?? []).filter((x) => x.id !== args.pointId); emitLocal("savepoints-changed", null); return r(undefined);
    case "export_candidates": return r([{ path: "config", size: 182_000, files: 41, suggested: true }, { path: "options.txt", size: 5_200, files: 1, suggested: false }, { path: "saves", size: 48_000_000, files: 812, suggested: false }]);
    case "export_instance": await sleep(1200); return r({ path: args.options.dest, size: 214_000, linked: 3, bundled: ["Just Enough Items"] });
    case "import_discover": await sleep(500); return r([
      { from: "prism", name: "All the Mods 10", gameDir: "/home/elchi/.local/share/PrismLauncher/instances/ATM10/.minecraft", gameVersion: "1.21.1", loader: "neoforge", loaderVersion: "21.1.77", mods: 412, worlds: 3, memoryMb: 10240 },
      { from: "prism", name: "Vanilla speedrun", gameDir: "/home/elchi/.local/share/PrismLauncher/instances/speedrun/.minecraft", gameVersion: "1.16.1", loader: "fabric", loaderVersion: "0.16.5", mods: 9, worlds: 41 },
      { from: "modrinthApp", name: "Cobblemon Official", gameDir: "/home/elchi/.local/share/ModrinthApp/profiles/Cobblemon Official", gameVersion: "1.21.1", loader: "fabric", loaderVersion: "0.16.10", mods: 73, worlds: 1 },
    ]);
    case "import_inspect": return r([]);
    case "curseforge_window": return r(undefined);
    case "pack_source": return r({ source: "modrinth", projectId: "1KVo5zza", versionId: null, name: "Fabulously Optimized", versionName: "6.4.0" });
    case "import_instance": await sleep(900); return r(db.instances[0]);
    case "crash_dismiss": delete crashes[args.instanceId]; return r(undefined);
    case "crash_apply_fix": {
      if (args.fix.kind === "disableMod") { const c = content.find((x) => x.path === args.fix.path); if (c) c.enabled = false; emitLocal("content-changed", null); }
      delete crashes[args.instanceId]; return r(undefined);
    }
    case "bisect_start": {
      const steps = [[5, 8], [3, 4], [2, 2], [1, 1]];
      for (const [i, [enabled, suspects]] of steps.entries()) {
        emitLocal("bisect-progress", { instanceId: args.instanceId, step: { trial: i + 1, estimate: 4, enabled, suspects } });
        await sleep(1500);
      }
      const found = { culprits: ["mods/iris.jar"], names: ["Iris Shaders"], alone: true, trials: 4 };
      emitLocal("bisect-progress", { instanceId: args.instanceId, found });
      return r(found);
    }
    case "alloymc_profile": return r(db.alloymc);
    case "alloymc_sign_in": {
      await sleep(1500);
      db.alloymc = { handle: "elchi", displayName: "Elchi", avatarUrl: "https://api.alloymc.dev/v1/users/elchi/avatar.png", profileUrl: "https://alloymc.dev/@elchi", accent: "gold", followers: 12, following: 3, email: "elchi@example.com", emailVerified: true, minecraft: [] };
      emitLocal("alloymc-changed", null); return r(db.alloymc);
    }
    case "alloymc_cancel_sign_in": return r(undefined);
    case "alloymc_sign_out": db.alloymc = null; emitLocal("alloymc-changed", null); return r(undefined);
    case "alloymc_link_minecraft": {
      await sleep(900);
      const a = db.accounts.find((x) => x.id === args.accountId)!;
      if (a.kind !== "microsoft") throw "Only Microsoft accounts can be linked. Offline players can't prove who they are.";
      db.alloymc?.minecraft.push({ uuid: a.id, name: a.username, shown: true });
      emitLocal("alloymc-changed", null); return r({ uuid: a.id, name: a.username, shown: true });
    }
    case "alloymc_unlink_minecraft": if (db.alloymc) db.alloymc.minecraft = db.alloymc.minecraft.filter((m) => m.uuid !== args.uuid); emitLocal("alloymc-changed", null); return r(undefined);
    case "list_accounts": return r({ accounts: db.accounts, active: db.active });
    case "add_offline_account": { const a: Account = { id: crypto.randomUUID().replace(/-/g, ""), kind: "offline", username: args.name, addedAt: now, needsLogin: false }; db.accounts.push(a); db.active = a.id; emitLocal("accounts-changed", null); return r(a); }
    case "login_microsoft": await sleep(1200); throw "The Microsoft login window opens in the desktop app.";
    case "remove_account": db.accounts = db.accounts.filter((a) => a.id !== args.id); if (db.active === args.id) db.active = db.accounts[0]?.id ?? null; emitLocal("accounts-changed", null); return r(undefined);
    case "set_active_account": db.active = args.id; emitLocal("accounts-changed", null); return r(undefined);
    case "search_content":
      return r(await modrinthSearch(args));
    case "modrinth_versions": {
      const res = await fetch(`https://api.modrinth.com/v2/project/${args.projectId}/version`);
      return r(await res.json());
    }
    case "install_modrinth_version": await sleep(900); emitLocal("content-changed", null); return r(["Installed"]);
    case "install_modrinth_modpack":
    case "import_modpack": await fakeJob(args.jobId, null); throw "Modpack installs run in the desktop app.";
    case "content_overview": return r({ items: args.instanceId === "create-above-beyond" ? bigPack() : content, report: { problems: [], requires: {}, requiredBy: {}, orphans: [], modCount: content.length }, blocked: [], listedMissing: false });
    case "set_content_enabled": { content.filter((x) => args.paths.includes(x.path)).forEach((c) => (c.enabled = args.enabled)); emitLocal("content-changed", null); return r(undefined); }
    case "remove_content": { for (const p of args.paths) { const idx = content.findIndex((x) => x.path === p); if (idx >= 0) content.splice(idx, 1); } emitLocal("content-changed", null); return r(undefined); }
    case "check_content_updates": case "content_fixes": case "list_jobs": return r([]);
    case "preflight": return r({ errors: [], warnings: [], system: [] });
    case "memory_advice": return r({ totalMb: 32768, maxGameMb: 28672, recommendedMb: args.instanceId ? 4096 : null, mods: args.instanceId ? 4 : null });
    case "instance_icon": case "content_icon": return r(null);
    case "assistant_status": return r({ signedIn: true, available: true, dayUsed: 12, monthUsed: 4, unlimited: false });
    case "assistant_send": return r(await mockAssistant(args.requestId, args.messages));
    case "assistant_cancel": case "assistant_checkpoint": return r(undefined);
    case "assistant_read_file": return r('{\n  "quality": { "weather_quality": "DEFAULT" }\n}');
    case "assistant_current_text": return r('{\n  "quality": {\n    "weather_quality": "DEFAULT",\n    "leaves_quality": "DEFAULT"\n  },\n  "advanced": {}\n}');
    case "assistant_write_file": await sleep(300); return r(undefined);
    case "assistant_read_log": return r("[12:00:01] [main/INFO]: Loading Minecraft 1.21.1 with Fabric Loader 0.20.1\n[12:00:09] [Render thread/INFO]: Sodium loaded");
    case "assistant_chats": return r(chats.map(({ messages: _m, labels: _l, ...c }) => c));
    case "assistant_chat": return r(chats.find((c) => c.id === args.id));
    case "assistant_chat_save": chats = [args.chat, ...chats.filter((c) => c.id !== args.chat.id)]; return r(undefined);
    case "assistant_chat_delete": chats = chats.filter((c) => c.id !== args.id); return r(undefined);
    case "test_launch": await sleep(2500); return r({ started: true });
  }
  throw `mock: unknown command ${cmd}`;
}
