// What right-clicking things offers. Each builder returns the items for one kind of thing,
// so the same instance gets the same menu on Home, in the Library and on its own page.

import { BookOpen, Bookmark, Copy, ExternalLink, FolderInput, FolderOpen, Play, PowerOff, Power, Server, Sparkles, Trash2, Wrench } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { MenuItem } from "../components/ContextMenu";
import { api, isTauri, newJobId } from "./api";
import { toastError, useStore } from "./store";
import { useAssistant } from "./assistant";
import type { ContentHit, ContentItem, Instance } from "./types";

export const openExternal = (url: string) => (isTauri ? openUrl(url) : Promise.resolve(void window.open(url, "_blank"))).catch(toastError);
const copy = (text: string) => navigator.clipboard.writeText(text).then(() => useStore.getState().toast({ kind: "success", text: "Copied" })).catch(() => {});
const sep: MenuItem = { separator: true, label: "" };

export function instanceMenu(i: Instance): MenuItem[] {
  const { go, toast } = useStore.getState();
  const running = !!useStore.getState().game[i.id];
  return [
    running
      ? { label: "Stop", icon: PowerOff, run: () => api.stop(i.id).catch(toastError) }
      : { label: "Play", icon: Play, run: () => api.launch(i.id).catch(toastError) },
    { label: "Open", icon: FolderInput, run: () => go({ name: "instance", id: i.id }) },
    { label: "Save points", icon: Bookmark, run: () => go({ name: "instance", id: i.id, tab: "savepoints" }) },
    ...(i.linkedPack?.name ? [{ label: "About this pack", icon: BookOpen, run: () => void openPackPage(i) }] : []),
    {
      label: "Ask the assistant",
      icon: Sparkles,
      run: () => {
        const a = useAssistant.getState();
        if (!a.busy) {
          if (a.messages.length) a.reset();
          a.setInstance(i.id);
        }
        a.setOpen(true);
      },
    },
    sep,
    { label: "Open folder", icon: FolderOpen, run: () => api.openFolder(i.id).catch(toastError) },
    { label: "Duplicate", icon: Copy, run: () => api.duplicateInstance(i.id, `${i.name} copy`).catch(toastError) },
    { label: "Make a server…", icon: Server, run: () => useStore.getState().setServerPack(i.id) },
    { label: "Repair files", icon: Wrench, run: () => api.installInstance(i.id, newJobId(), true).catch(toastError) },
    sep,
    {
      label: "Move to trash",
      icon: Trash2,
      danger: true,
      disabled: running,
      run: () =>
        api
          .deleteInstance(i.id)
          .then(() => toast({ kind: "info", text: `${i.name} moved to the launcher's trash folder` }))
          .catch(toastError),
    },
  ];
}

export function contentPageUrl(item: ContentItem): string | null {
  const id = item.meta?.projectId;
  if (!id) return null;
  if (item.meta?.source === "modrinth") return `https://modrinth.com/project/${id}`;
  if (item.meta?.source === "curseforge") return `https://www.curseforge.com/projects/${id}`;
  return null;
}

/** For a mod, resource pack or shader in an instance. `selection` is set when several are selected. */
export function contentMenu(instance: Instance, item: ContentItem, opts: { select: () => void; selection?: string[]; remove: () => void }): MenuItem[] {
  const go = useStore.getState().go;
  const many = opts.selection && opts.selection.length > 1 && opts.selection.includes(item.path) ? opts.selection : null;
  if (many) {
    return [
      { label: `Turn on ${many.length}`, icon: Power, run: () => api.setContentEnabled(instance.id, many, true).catch(toastError) },
      { label: `Turn off ${many.length}`, icon: PowerOff, run: () => api.setContentEnabled(instance.id, many, false).catch(toastError) },
      sep,
      { label: `Remove ${many.length}`, icon: Trash2, danger: true, run: () => api.removeContent(instance.id, many).catch(toastError) },
    ];
  }
  const url = contentPageUrl(item);
  const source = item.meta?.source;
  const folder = item.path.split("/").slice(0, -1).join("/");
  const items: MenuItem[] = [
    item.enabled
      ? { label: "Turn off", icon: PowerOff, run: () => api.setContentEnabled(instance.id, [item.path], false).catch(toastError) }
      : { label: "Turn on", icon: Power, run: () => api.setContentEnabled(instance.id, [item.path], true).catch(toastError) },
  ];
  if (item.meta?.projectId && (source === "modrinth" || source === "curseforge")) {
    items.push({ label: "Project page", icon: FolderInput, run: () => go({ name: "project", platform: source, id: item.meta!.projectId!, target: instance.id }) });
  }
  if (url) items.push({ label: source === "modrinth" ? "Open on Modrinth" : "Open on CurseForge", icon: ExternalLink, run: () => openExternal(url) });
  items.push(
    { label: "Show in folder", icon: FolderOpen, run: () => api.openFolder(instance.id, folder).catch(toastError) },
    { label: "Copy name", icon: Copy, run: () => copy(item.title) },
    { label: "Select", run: opts.select },
    sep,
    { label: "Remove", icon: Trash2, danger: true, run: opts.remove },
  );
  return items;
}

export function hitPageUrl(hit: ContentHit): string {
  return hit.source === "modrinth" ? `https://modrinth.com/project/${hit.slug || hit.id}` : `https://www.curseforge.com/projects/${hit.id}`;
}

/** For a search result in Discover. */
export function hitMenu(hit: ContentHit, opts: { open: () => void; install?: () => void; installLabel?: string }): MenuItem[] {
  const url = hitPageUrl(hit);
  return [
    { label: "Open", icon: FolderInput, run: opts.open },
    ...(opts.install ? [{ label: opts.installLabel ?? "Install", icon: Play, run: opts.install }] : []),
    sep,
    { label: hit.source === "modrinth" ? "Open on Modrinth" : "Open on CurseForge", icon: ExternalLink, run: () => openExternal(url) },
    { label: "Copy link", icon: Copy, run: () => copy(url) },
  ];
}

/** Opens the page of the public pack an instance came from, finding it first if needed. */
export async function openPackPage(instance: Instance) {
  const { go, toast } = useStore.getState();
  try {
    const link = await api.packSource(instance.id);
    if (link?.projectId && (link.source === "modrinth" || link.source === "curseforge")) {
      go({ name: "project", platform: link.source, id: link.projectId });
    } else {
      toast({ kind: "info", text: `${instance.linkedPack?.name ?? "This pack"} isn't on Modrinth or CurseForge, as far as Alloy can tell.` });
    }
  } catch (e) {
    toastError(e);
  }
}
