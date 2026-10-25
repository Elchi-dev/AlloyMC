import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../lib/api";
import { useSettings } from "../lib/queries";
import { queryClient } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { Settings } from "../lib/types";
import { Segmented } from "../components/ui";
import { open as openFile } from "@tauri-apps/plugin-dialog";
import { applyUpdate } from "../components/Updates";
import { isTauri } from "../lib/api";

export function SettingsPage() {
  const { data } = useSettings();
  const info = useQuery({ queryKey: ["appInfo"], queryFn: api.appInfo });
  const [draft, setDraft] = useState<Settings | null>(null);
  useEffect(() => {
    if (data) setDraft(data);
  }, [data]);
  if (!draft) return null;
  const dirty = JSON.stringify(draft) !== JSON.stringify(data);
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setDraft({ ...draft, [k]: v });

  const save = async () => {
    try {
      await api.saveSettings(draft);
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      useStore.getState().toast({ kind: "success", text: "Settings saved" });
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-9 px-8 py-8">
      <Section title="Look and feel" index={0}>
        <Row label="Motion" hint="Full animates everything. Calm keeps transitions and slows loops. Off makes every change instant.">
          <Segmented
            label="Motion"
            value={draft.motion}
            onChange={(v) => set("motion", v)}
            options={[
              { value: "full", label: "Full" },
              { value: "calm", label: "Calm" },
              { value: "off", label: "Off" },
            ]}
          />
        </Row>
        <Row label="Background" hint="The forge pauses while you play, so it never costs you frames.">
          <Segmented
            label="Background"
            value={draft.background}
            onChange={(v) => set("background", v)}
            options={[
              { value: "forge", label: "Alloy forge" },
              { value: "plain", label: "Plain" },
            ]}
          />
        </Row>
      </Section>

      <Section title="Playing" index={1}>
        <Row label="Default memory" hint={`${(draft.defaultMemoryMb / 1024).toFixed(1)} GB for instances that don't set their own.`}>
          <input type="range" min={1024} max={16384} step={512} value={draft.defaultMemoryMb} onChange={(e) => set("defaultMemoryMb", Number(e.target.value))} className="w-full accent-[var(--color-gold)]" />
        </Row>
        <Row label="When a game starts">
          <Segmented
            label="When a game starts"
            value={draft.onLaunch}
            onChange={(v) => set("onLaunch", v)}
            options={[
              { value: "keep_open", label: "Keep open" },
              { value: "minimize", label: "Minimize" },
              { value: "close", label: "Hide until it closes" },
            ]}
          />
        </Row>
        <Row label="Parallel downloads" hint="Lower this on slow or metered connections.">
          <input type="number" min={1} max={64} className="field w-24" value={draft.maxConcurrentDownloads} onChange={(e) => set("maxConcurrentDownloads", Number(e.target.value) || 1)} />
        </Row>
      </Section>

      <Section title="Services" index={2}>
        <Row
          label="CurseForge API key"
          hint={info.data?.curseforgeBuiltin ? "This build has a key built in. Paste another one here to override it." : "CurseForge is a work in progress and arrives with the Alloy server. Paste your own key from console.curseforge.com to try it now."}
        >
          <input type="password" className="field font-mono text-[12px]" placeholder={info.data?.curseforgeBuiltin ? "Built-in key" : "Your key"} value={draft.curseforgeApiKey ?? ""} onChange={(e) => set("curseforgeApiKey", e.target.value || null)} />
        </Row>
        <Row label="Microsoft client ID" hint="Leave empty to sign in like the official launcher. Set your own Azure app ID only if sign-in ever stops working.">
          <input className="field font-mono text-[12px]" placeholder="Official launcher (default)" value={draft.customMsaClientId ?? ""} onChange={(e) => set("customMsaClientId", e.target.value || null)} />
        </Row>
      </Section>

      <UpdatesSection version={info.data?.version} />

      <Section title="About" index={4}>
        <Row label="Version">
          <span className="block pt-2 text-ink-2">{info.data?.version}</span>
        </Row>
        <Row label="Data folder">
          <span className="block break-all pt-2 font-mono text-[12px] text-ink-2" data-selectable>{info.data?.dataDir}</span>
        </Row>
      </Section>

      <div className="sticky bottom-0 flex justify-end gap-2 border-t border-line-soft bg-slate-950/95 py-4">
        <button className="btn btn-quiet" disabled={!dirty} onClick={() => setDraft(data!)}>Discard</button>
        <button className="btn btn-gold" disabled={!dirty} onClick={save}>Save changes</button>
      </div>
    </div>
  );
}

function Section({ title, children, index }: { title: string; children: React.ReactNode; index: number }) {
  return (
    <section className="panel rise rounded-[12px] p-6" style={{ ["--i" as string]: index } as React.CSSProperties}>
      <h2 className="mb-4 border-b border-line-soft pb-2 text-[15px] font-semibold">{title}</h2>
      <div className="flex flex-col gap-6">{children}</div>
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[200px_1fr] items-start gap-4">
      <span className="pt-2 text-[13px] font-medium text-ink-2">{label}</span>
      <div>
        {children}
        {hint && <p className="mt-1.5 text-[12px] text-ink-3">{hint}</p>}
      </div>
    </div>
  );
}

function UpdatesSection({ version }: { version?: string }) {
  const update = useStore((s) => s.update);
  const setUpdate = useStore((s) => s.setUpdate);
  const pick = async () => {
    const path = await openFile({ filters: [{ name: "Alloy update", extensions: ["alloyupdate"] }] });
    if (typeof path !== "string") return;
    try {
      setUpdate(await api.inspectUpdate(path));
    } catch (e) {
      toastError(e);
    }
  };
  return (
    <Section title="Updates" index={3}>
      <Row label="Installed" hint="Alloy looks for .alloyupdate files in your Downloads folder whenever it opens or regains focus. You can also drop one onto the window.">
        <span className="block pt-2 text-ink-2">Alloy {version}</span>
      </Row>
      <Row label="Available">
        {update ? (
          <div className="rise flex flex-col items-start gap-2 pt-1">
            <span className="text-ink">
              Alloy {update.manifest.version} <span className="text-ink-3">{update.newer ? "(signature verified)" : "(not newer than yours)"}</span>
            </span>
            {update.manifest.notes && <pre className="max-h-40 overflow-auto whitespace-pre-wrap font-sans text-[12px] text-ink-2">{update.manifest.notes}</pre>}
            <button className="btn btn-gold" onClick={() => applyUpdate(update.path, !update.newer)}>
              {update.newer ? "Restart and update" : "Install this version anyway"}
            </button>
          </div>
        ) : (
          <div className="flex items-center gap-3 pt-1">
            <span className="text-ink-3">No update found</span>
            <button className="btn" onClick={pick} disabled={!isTauri}>Install update from file…</button>
          </div>
        )}
      </Row>
    </Section>
  );
}
