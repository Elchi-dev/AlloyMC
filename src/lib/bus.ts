// Backend events, with the same shape from Tauri or the browser mock.

import { listen as tauriListen } from "@tauri-apps/api/event";
import { isTauri } from "./api";

type Handler = (payload: any) => void;
const local = new Map<string, Set<Handler>>();

/** Used by the mock to emit events in the browser. */
export function emitLocal(event: string, payload: unknown) {
  local.get(event)?.forEach((h) => h(payload));
}

export function listen(event: string, handler: Handler): () => void {
  if (isTauri) {
    const p = tauriListen(event, (e) => handler(e.payload));
    return () => void p.then((un) => un());
  }
  if (!local.has(event)) local.set(event, new Set());
  local.get(event)!.add(handler);
  return () => local.get(event)!.delete(handler);
}
