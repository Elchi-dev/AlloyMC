import { useCallback, useMemo, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ChevronDown, Copy, FolderInput, FolderOpen, FolderPlus, Pencil, Play, Plus, Search, Trash2, Wrench } from "lucide-react";
import { InstanceIcon } from "../components/Art";
import { ContextMenu, type MenuItem } from "../components/ContextMenu";
import { PlayButton } from "../components/PlayButton";
import { Dialog, Empty, Segmented } from "../components/ui";
import { api, newJobId } from "../lib/api";
import { ago, loaderName, progressFraction, progressLabel } from "../lib/format";
import { useInstances } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { Instance } from "../lib/types";

type Sort = "played" | "name" | "created";

const COLLAPSED_KEY = "alloy-collapsed-groups";
function loadCollapsed(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]"));
  } catch {
    return new Set();
  }
}

export function LibraryPage() {
  const { data } = useInstances();
  const setCreateOpen = useStore((s) => s.setCreateOpen);
  const [q, setQ] = useState("");
  const [sort, setSort] = useState<Sort>("played");
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const [menu, setMenu] = useState<{ at: { x: number; y: number }; instance: Instance } | null>(null);
  const [rename, setRename] = useState<Instance | null>(null);
  const [newGroupFor, setNewGroupFor] = useState<Instance | null>(null);

  const list = useMemo(() => {
    const term = q.trim().toLowerCase();
    const filtered = (data ?? []).filter(
      (i) => !term || i.name.toLowerCase().includes(term) || i.gameVersion.includes(term) || i.loader.includes(term) || (i.group ?? "").toLowerCase().includes(term),
    );
    return filtered.sort((a, b) => (sort === "name" ? a.name.localeCompare(b.name) : sort === "created" ? b.createdAt - a.createdAt : (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0)));
  }, [data, q, sort]);

  const groups = useMemo(() => {
    const names = [...new Set(list.map((i) => i.group).filter(Boolean) as string[])].sort((a, b) => a.localeCompare(b));
    const out: { name: string | null; items: Instance[] }[] = names.map((n) => ({ name: n, items: list.filter((i) => i.group === n) }));
    const rest = list.filter((i) => !i.group);
    if (rest.length) out.push({ name: null, items: rest });
    return out;
  }, [list]);
  const allGroups = useMemo(() => [...new Set((data ?? []).map((i) => i.group).filter(Boolean) as string[])].sort(), [data]);
  const grouped = groups.some((g) => g.name);

  const toggle = (name: string) => {
    setCollapsed((c) => {
      const n = new Set(c);
      n.has(name) ? n.delete(name) : n.add(name);
      try {
        localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...n]));
      } catch {
        // per-viewer convenience only
      }
      return n;
    });
  };

  const setGroup = (inst: Instance, group: string | null) => api.updateInstance({ ...inst, group }).catch(toastError);
  const openMenu = useCallback((e: React.MouseEvent, instance: Instance) => {
    e.preventDefault();
    setMenu({ at: { x: e.clientX, y: e.clientY }, instance });
  }, []);
  const menuItems = (i: Instance): MenuItem[] => {
    const go = useStore.getState().go;
    const toast = useStore.getState().toast;
    return [
      { label: "Play", icon: Play, run: () => api.launch(i.id).catch(toastError) },
      { label: "Open", icon: FolderInput, run: () => go({ name: "instance", id: i.id }) },
      { separator: true, label: "" },
      { label: "Rename", icon: Pencil, run: () => setRename(i) },
      { label: "Duplicate", icon: Copy, run: () => api.duplicateInstance(i.id, `${i.name} copy`).catch(toastError) },
      {
        label: "Move to group",
        icon: FolderPlus,
        children: [
          ...allGroups.filter((g) => g !== i.group).map((g) => ({ label: g, run: () => setGroup(i, g) })),
          { label: "New group…", run: () => setNewGroupFor(i) },
          ...(i.group ? [{ label: "No group", run: () => setGroup(i, null) }] : []),
        ],
      },
      { label: "Open folder", icon: FolderOpen, run: () => api.openFolder(i.id).catch(toastError) },
      { label: "Repair files", icon: Wrench, run: () => api.installInstance(i.id, newJobId(), true).catch(toastError) },
      { separator: true, label: "" },
      {
        label: "Move to trash",
        icon: Trash2,
        danger: true,
        run: () =>
          api
            .deleteInstance(i.id)
            .then(() => toast({ kind: "info", text: `${i.name} moved to the launcher's trash folder` }))
            .catch(toastError),
      },
    ];
  };

  let index = 0;
  return (
    <div className="mx-auto flex max-w-[1100px] flex-col gap-5 px-8 py-6">
      <div className="rise flex items-center gap-3">
        <div className="relative w-72">
          <Search size={15} className="absolute left-2.5 top-2.5 text-ink-3" />
          <input className="field pl-8" placeholder="Search instances" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <Segmented
          label="Sort"
          value={sort}
          onChange={setSort}
          options={[
            { value: "played", label: "Last played" },
            { value: "name", label: "Name" },
            { value: "created", label: "Newest" },
          ]}
        />
        <button className="btn btn-gold ml-auto" onClick={() => setCreateOpen(true)}>
          <Plus size={15} /> New instance
        </button>
      </div>

      {data && !data.length ? (
        <Empty
          title="No instances yet"
          body="Create one from scratch, install a modpack from Discover, or drop a .mrpack or CurseForge zip onto this window."
          pose="hi"
          action={<button className="btn btn-gold" onClick={() => setCreateOpen(true)}>New instance</button>}
        />
      ) : (
        groups.map((g) => {
          const key = g.name ?? "";
          const isCollapsed = grouped && collapsed.has(key);
          return (
            <section key={key || "ungrouped"} className="flex flex-col gap-3">
              {grouped && (
                <button className="rise flex items-center gap-2 text-left" onClick={() => toggle(key)} aria-expanded={!isCollapsed}>
                  <ChevronDown size={15} className={`text-ink-3 transition-transform duration-200 ${isCollapsed ? "-rotate-90" : ""}`} />
                  <h3 className="text-[13px] font-semibold uppercase tracking-wide text-ink-2">{g.name ?? "Other"}</h3>
                  <span className="text-[12px] tabular-nums text-ink-3">{g.items.length}</span>
                  <span className="ml-2 h-px flex-1 bg-line-soft" />
                </button>
              )}
              <AnimatePresence initial={false}>
                {!isCollapsed && (
                  <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }} className="overflow-hidden">
                    <div className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-3 p-0.5">
                      {g.items.map((inst) => (
                        <Tile key={inst.id} instance={inst} index={index++} onContextMenu={openMenu} />
                      ))}
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </section>
          );
        })
      )}
      {data && data.length > 0 && !list.length && <p className="fade-in text-ink-3">Nothing matches “{q}”.</p>}

      <ContextMenu at={menu?.at ?? null} items={menu ? menuItems(menu.instance) : []} onClose={() => setMenu(null)} />
      <NameDialog
        open={!!rename}
        title="Rename instance"
        initial={rename?.name ?? ""}
        confirm="Rename"
        onClose={() => setRename(null)}
        onSubmit={(name) => rename && api.updateInstance({ ...rename, name }).catch(toastError)}
      />
      <NameDialog
        open={!!newGroupFor}
        title="New group"
        initial=""
        placeholder="Modded, Survival, With friends…"
        confirm="Create group"
        onClose={() => setNewGroupFor(null)}
        onSubmit={(group) => newGroupFor && setGroup(newGroupFor, group)}
      />
    </div>
  );
}

function NameDialog({ open, title, initial, placeholder, confirm, onClose, onSubmit }: { open: boolean; title: string; initial: string; placeholder?: string; confirm: string; onClose: () => void; onSubmit: (v: string) => void }) {
  const [v, setV] = useState(initial);
  const [lastOpen, setLastOpen] = useState(false);
  if (open !== lastOpen) {
    setLastOpen(open);
    if (open) setV(initial);
  }
  return (
    <Dialog open={open} onClose={onClose} title={title} width={420}>
      <form
        className="flex flex-col gap-4 p-5"
        onSubmit={(e) => {
          e.preventDefault();
          if (v.trim()) {
            onSubmit(v.trim());
            onClose();
          }
        }}
      >
        <input autoFocus className="field" value={v} placeholder={placeholder} onChange={(e) => setV(e.target.value)} onFocus={(e) => e.currentTarget.select()} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn btn-quiet" onClick={onClose}>
            Cancel
          </button>
          <button className="btn btn-gold" disabled={!v.trim()}>
            {confirm}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

function Tile({ instance, index, onContextMenu }: { instance: Instance; index: number; onContextMenu: (e: React.MouseEvent, i: Instance) => void }) {
  const go = useStore((s) => s.go);
  const progress = useStore((s) => s.progress[instance.id]);
  const running = useStore((s) => s.game[instance.id] === "running");
  return (
    <div className="card rise group relative flex flex-col rounded-[12px]" style={{ ["--i" as string]: Math.min(index, 18) } as React.CSSProperties} onContextMenu={(e) => onContextMenu(e, instance)}>
      <button onClick={() => go({ name: "instance", id: instance.id })} className="flex items-center gap-3.5 p-4 pb-3 text-left">
        <InstanceIcon instance={instance} size={52} shared tilt />
        <span className="min-w-0">
          <span className="block truncate text-[15px] font-semibold">{instance.name}</span>
          <span className="block truncate text-[12px] text-ink-2">
            {loaderName[instance.loader]} {instance.gameVersion}
          </span>
        </span>
      </button>
      <div className="flex items-center gap-2 px-4 pb-4">
        <span className="min-w-0 flex-1 truncate text-[12px] text-ink-3">
          {running ? (
            <span className="text-grass">Playing now</span>
          ) : progress ? (
            `${progressLabel(progress)} ${Math.round(progressFraction(progress) * 100)}%`
          ) : instance.installState === "failed" ? (
            <span className="text-redstone">Install failed</span>
          ) : instance.installState === "not_installed" ? (
            "Installs on first play"
          ) : (
            ago(instance.lastPlayed)
          )}
        </span>
        <PlayButton instance={instance} size="sm" />
      </div>
      {progress && (
        <div className="absolute inset-x-4 bottom-0 h-[2px] overflow-hidden rounded bg-line">
          <div className="play-progress h-full bg-gold transition-[width]" style={{ width: `${progressFraction(progress) * 100}%` }} />
        </div>
      )}
    </div>
  );
}
