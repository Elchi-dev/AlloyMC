import { useEffect } from "react";
import { QueryClient, useQuery } from "@tanstack/react-query";
import { api } from "./api";
import { listen } from "./bus";
import { useStore, type LogLine } from "./store";
import type { GameStateEvent, Progress } from "./types";

export const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 30_000, refetchOnWindowFocus: false, retry: 1 } },
});

export const useInstances = () => useQuery({ queryKey: ["instances"], queryFn: api.instances });
export const useInstance = (id: string) => useQuery({ queryKey: ["instances", id], queryFn: () => api.instance(id) });
export const useAccounts = () => useQuery({ queryKey: ["accounts"], queryFn: api.accounts });
export const useSettings = () => useQuery({ queryKey: ["settings"], queryFn: api.getSettings });
export const useContent = (id: string) => useQuery({ queryKey: ["content", id], queryFn: () => api.content(id) });
export const useGameVersions = (snapshots: boolean) =>
  useQuery({ queryKey: ["versions", snapshots], queryFn: () => api.gameVersions(snapshots), staleTime: 600_000 });

// Mojang's log config prints log4j XML events; fold each into one readable line.
const xmlBuffers: Record<string, string[]> = {};
function parseLine(id: string, stream: "stdout" | "stderr", raw: string): LogLine | null {
  const buf = xmlBuffers[id];
  if (raw.trimStart().startsWith("<log4j:Event")) {
    xmlBuffers[id] = [raw];
    return null;
  }
  if (buf) {
    buf.push(raw);
    if (!raw.includes("</log4j:Event>")) return null;
    delete xmlBuffers[id];
    const xml = buf.join("\n");
    const level = (/level="(\w+)"/.exec(xml)?.[1] ?? "INFO").toLowerCase();
    const thread = /thread="([^"]+)"/.exec(xml)?.[1] ?? "";
    const msg = /<!\[CDATA\[([\s\S]*?)\]\]>/.exec(xml)?.[1] ?? "";
    const time = Number(/timestamp="(\d+)"/.exec(xml)?.[1] ?? 0);
    const t = time ? new Date(time).toLocaleTimeString([], { hour12: false }) : "";
    return { stream, level: normLevel(level), line: `[${t}] [${thread}/${level.toUpperCase()}]: ${msg}` };
  }
  const m = /\/(INFO|WARN|ERROR|DEBUG|FATAL)\]/.exec(raw);
  const level = m ? normLevel(m[1].toLowerCase()) : stream === "stderr" ? "warn" : "info";
  return { stream, level, line: raw };
}
function normLevel(l: string): LogLine["level"] {
  if (l === "warn") return "warn";
  if (l === "error" || l === "fatal") return "error";
  if (l === "debug" || l === "trace") return "debug";
  return "info";
}

/** Wires backend events into the store and the query cache. Mount once. */
export function useBackendEvents() {
  useEffect(() => {
    const s = useStore.getState;
    const offs = [
      listen("job-progress", (p: { jobId: string; instanceId: string | null; progress: Progress }) =>
        s().setProgress(p.jobId, p.instanceId, p.progress),
      ),
      listen("instances-changed", () => queryClient.invalidateQueries({ queryKey: ["instances"] })),
      listen("accounts-changed", () => queryClient.invalidateQueries({ queryKey: ["accounts"] })),
      listen("content-changed", () => queryClient.invalidateQueries({ queryKey: ["content"] })),
      listen("game-state", (e: GameStateEvent) => {
        if (e.state === "exited") {
          s().setGame(e.instanceId, undefined);
          s().clearProgress(e.instanceId);
          if (e.error) s().toast({ kind: "error", text: e.error });
          else if (e.exitCode && e.exitCode !== 0) {
            s().toast({
              kind: "error",
              text: `The game crashed (exit code ${e.exitCode}).`,
              action: { label: "Show log", run: () => s().go({ name: "instance", id: e.instanceId, tab: "logs" }) },
            });
          }
        } else {
          if (e.state === "starting") s().clearLogs(e.instanceId);
          s().setGame(e.instanceId, e.state);
        }
      }),
      listen("game-log", (e: { instanceId: string; stream: "stdout" | "stderr"; line: string }) => {
        const parsed = parseLine(e.instanceId, e.stream, e.line);
        if (parsed) s().pushLog(e.instanceId, parsed);
      }),
    ];
    api.running().then((ids) => ids.forEach((id) => s().setGame(id, "running")));
    return () => offs.forEach((off) => off());
  }, []);
}
