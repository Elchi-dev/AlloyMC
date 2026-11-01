// Making a dedicated server from an instance: what goes in, where it goes, and how to start it.

import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { Check, Copy, FileCog, FolderOpen, Package, Server, ShieldCheck, Terminal } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, isTauri, type ServerPackReport } from "../lib/api";
import { listen } from "../lib/bus";
import { loaderName } from "../lib/format";
import { openExternal } from "../lib/menus";
import { useInstance, useOverview } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import { Dialog } from "./ui";
import { Ori } from "./ori/Ori";

export function ServerPackDialog() {
  const id = useStore((s) => s.serverPack);
  const close = () => useStore.getState().setServerPack(null);
  return (
    <Dialog open={!!id} onClose={close} title="Make a server" width={560}>
      {id && <Body key={id} instanceId={id} onClose={close} />}
    </Dialog>
  );
}

function Body({ instanceId, onClose }: { instanceId: string; onClose: () => void }) {
  const { data: inst } = useInstance(instanceId);
  const { data: ov } = useOverview(instanceId);
  const [dest, setDest] = useState<string | null>(null);
  const [eula, setEula] = useState(false);
  const [busy, setBusy] = useState(false);
  const [step, setStep] = useState("");
  const [report, setReport] = useState<ServerPackReport | null>(null);
  const mods = ov?.items.filter((x) => x.kind === "mod" && x.enabled).length ?? 0;

  useEffect(
    () =>
      listen("job-progress", (p: { jobId: string; progress: { message?: string | null } }) => {
        if (p.jobId === `server:${instanceId}` && p.progress.message) setStep(p.progress.message);
      }),
    [instanceId],
  );

  const pick = async () => {
    const d = isTauri ? await openDialog({ directory: true, title: "Choose an empty folder for the server" }) : "~/Servers/my-server";
    if (typeof d === "string") setDest(d);
  };
  const build = async () => {
    if (!dest) return;
    setBusy(true);
    setStep("Starting");
    try {
      setReport(await api.makeServerPack(instanceId, dest, eula));
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };

  if (!inst) return null;
  if (report) return <Done report={report} onClose={onClose} />;

  return (
    <div className="flex flex-col gap-5 p-5">
      <div className="flex items-start gap-4">
        <Ori pose={busy ? "working" : "planning"} size={64} />
        <div className="min-w-0 flex-1">
          <h3 className="text-[16px] font-semibold text-ink">A server for {inst.name}</h3>
          <p className="mt-1 text-[13px] leading-relaxed text-ink-3">For playing together: run it on a PC or a host that stays on, and your friends join with the same pack.</p>
        </div>
      </div>

      <ul className="grid grid-cols-2 gap-2">
        <What icon={Server} title={`Minecraft ${inst.gameVersion}`} body={`with ${loaderName[inst.loader]}${inst.loaderVersion ? ` ${inst.loaderVersion}` : ""}, the same as the instance`} />
        <What icon={Package} title={`${mods} mods`} body="minus the ones only a client runs, like shaders and Sodium" />
        <What icon={FileCog} title="Configs and scripts" body="so the server plays like your instance" />
        <What icon={Terminal} title="Start scripts" body="for Windows, Linux and macOS" />
      </ul>

      <div>
        <p className="mb-1.5 text-[12.5px] font-medium text-ink-2">Folder</p>
        <div className="flex gap-2">
          <button className="field flex h-9 min-w-0 flex-1 items-center truncate text-left text-[13px]" onClick={pick} disabled={busy}>
            {dest ? <span className="truncate text-ink">{dest}</span> : <span className="text-ink-3">Choose an empty folder…</span>}
          </button>
          <button className="btn" onClick={pick} disabled={busy}>
            <FolderOpen size={14} /> Choose
          </button>
        </div>
      </div>

      <label className="flex cursor-pointer items-start gap-2.5 rounded-[10px] border border-line-soft bg-slate-900/60 p-3">
        <input type="checkbox" className="ai-check mt-0.5" checked={eula} onChange={() => setEula(!eula)} disabled={busy} />
        <span className="text-[12.5px] leading-relaxed text-ink-2">
          I accept Minecraft's{" "}
          <button className="text-gold underline underline-offset-2" onClick={(e) => (e.preventDefault(), openExternal("https://aka.ms/MinecraftEULA"))}>
            End User License Agreement
          </button>
          . A server won't start until it's accepted; you can also do it later in eula.txt.
        </span>
      </label>

      <div className="flex items-center gap-3 border-t border-line-soft pt-4">
        {busy ? (
          <p className="flex min-w-0 flex-1 items-center gap-2 truncate text-[12.5px] text-ink-3">
            <span className="spinner" /> {step || "Working"}
          </p>
        ) : (
          <span className="flex-1" />
        )}
        <button className="btn btn-quiet" onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button className="btn btn-gold" onClick={build} disabled={!dest || busy}>
          <Server size={14} /> Make server
        </button>
      </div>
    </div>
  );
}

function What({ icon: Icon, title, body }: { icon: typeof Server; title: string; body: string }) {
  return (
    <li className="flex gap-2.5 rounded-[10px] border border-line-soft bg-slate-900/60 p-3">
      <Icon size={15} className="mt-0.5 shrink-0 text-gold" />
      <span className="min-w-0">
        <span className="block text-[13px] font-medium text-ink">{title}</span>
        <span className="block text-[12px] leading-snug text-ink-3">{body}</span>
      </span>
    </li>
  );
}

function Done({ report, onClose }: { report: ServerPackReport; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  const modern = report.start.startsWith("run.sh");
  const windows = navigator.userAgent.includes("Windows");
  const command = windows ? (modern ? "run.bat" : "start.bat") : modern ? "./run.sh" : "./start.sh";
  return (
    <motion.div initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} className="flex flex-col gap-4 p-5">
      <div className="flex items-start gap-4">
        <Ori pose="done" size={64} />
        <div className="min-w-0 flex-1">
          <h3 className="text-[16px] font-semibold text-ink">The server is ready</h3>
          <p className="mt-1 truncate text-[12.5px] text-ink-3" title={report.dir}>
            {report.dir}
          </p>
        </div>
      </div>

      <div className="rounded-[10px] border border-line-soft bg-slate-900/60 p-3">
        <p className="text-[12.5px] font-medium text-ink">Start it</p>
        <p className="mt-0.5 text-[12px] text-ink-3">
          Needs Java {report.javaMajor}. Run {report.start} in the folder.
        </p>
        <div className="mt-2 flex items-center gap-2 rounded-[8px] bg-slate-950 px-3 py-2 font-mono text-[12px] text-ink">
          <span className="min-w-0 flex-1 truncate">{command}</span>
          <button
            className="text-ink-3 transition-colors hover:text-ink"
            aria-label="Copy"
            onClick={() => void navigator.clipboard.writeText(command).then(() => (setCopied(true), setTimeout(() => setCopied(false), 1400)))}
          >
            {copied ? <Check size={13} className="text-grass" /> : <Copy size={13} />}
          </button>
        </div>
      </div>

      <div className="flex gap-2">
        <Stat value={report.mods} label="mods on the server" />
        <Stat value={report.leftOut.length} label="left out, client only" />
      </div>
      {report.leftOut.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {report.leftOut.map((n) => (
            <span key={n} className="rounded-full border border-line bg-slate-800/80 px-2 py-[2px] text-[11.5px] text-ink-2">
              {n}
            </span>
          ))}
        </div>
      )}
      {!report.eulaAccepted && (
        <p className="flex items-start gap-2 rounded-[10px] border border-molten/30 bg-molten/[0.07] p-3 text-[12.5px] leading-relaxed text-ink-2">
          <ShieldCheck size={14} className="mt-0.5 shrink-0 text-molten" /> Before the first start, open eula.txt and change eula=false to eula=true to accept Minecraft's EULA.
        </p>
      )}

      <div className="flex justify-end gap-2 border-t border-line-soft pt-4">
        <button className="btn btn-quiet" onClick={() => revealItemInDir(`${report.dir}/eula.txt`).catch(toastError)}>
          <FolderOpen size={14} /> Open folder
        </button>
        <button className="btn btn-gold" onClick={onClose}>
          Done
        </button>
      </div>
    </motion.div>
  );
}

function Stat({ value, label }: { value: number; label: string }) {
  return (
    <div className="flex-1 rounded-[10px] border border-line-soft bg-slate-900/60 px-3 py-2.5">
      <p className="font-pixel text-[22px] leading-none text-ink">{value}</p>
      <p className="mt-1 text-[12px] text-ink-3">{label}</p>
    </div>
  );
}
