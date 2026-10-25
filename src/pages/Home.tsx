import type { CSSProperties } from "react";
import { Compass, FolderInput, Plus } from "lucide-react";
import { InstanceIcon } from "../components/Art";
import { PlayButton } from "../components/PlayButton";
import { Ori } from "../components/ori/Ori";
import { ago, loaderName, playTime } from "../lib/format";
import { useAccounts, useInstances } from "../lib/queries";
import { useStore } from "../lib/store";
import type { Instance } from "../lib/types";

const i = (n: number) => ({ ["--i" as string]: n }) as CSSProperties;

export function HomePage() {
  const { data: instances, isLoading } = useInstances();
  const { data: accounts } = useAccounts();
  const go = useStore((s) => s.go);
  const setCreateOpen = useStore((s) => s.setCreateOpen);
  const active = accounts?.accounts.find((a) => a.id === accounts.active);

  if (isLoading) return null;
  const [last, ...rest] = instances ?? [];

  return (
    <div className="mx-auto flex min-h-full max-w-[1120px] flex-col px-8 pb-8">
      {/* The forge shows through this space. */}
      <div className="min-h-[clamp(140px,30vh,340px)] flex-1" />

      {last ? <Hero instance={last} player={active?.username} /> : <FirstRun hasAccount={!!active} />}

      {rest.length > 0 && (
        <section className="mt-9">
          <div className="rise mb-3 flex items-baseline" style={i(4)}>
            <h2 className="text-[15px] font-semibold">Jump back in</h2>
            <button className="ml-auto text-[13px] text-ink-2 transition-colors hover:text-ink" onClick={() => go({ name: "library" })}>
              All {instances!.length} instances
            </button>
          </div>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3">
            {rest.slice(0, 6).map((inst, n) => (
              <RecentTile key={inst.id} instance={inst} index={5 + n} />
            ))}
          </div>
        </section>
      )}

      <section className="mt-3 grid grid-cols-3 gap-3">
        <QuickAction index={11} icon={<Plus size={18} />} title="New instance" body="Any version, any loader" onClick={() => setCreateOpen(true)} />
        <QuickAction index={12} icon={<Compass size={18} />} title="Find a modpack" body="Modrinth and CurseForge" onClick={() => go({ name: "discover", kind: "modpack" })} />
        <QuickAction index={13} icon={<FolderInput size={18} />} title="Import" body=".mrpack, CurseForge zip or a link" onClick={() => setCreateOpen(true)} />
      </section>
    </div>
  );
}

function Hero({ instance, player }: { instance: Instance; player?: string }) {
  const go = useStore((s) => s.go);
  return (
    <section className="flex items-end gap-8">
      <div className="min-w-0 flex-1">
        <p className="rise mb-4 text-[13px] text-ink-2 [text-shadow:0_1px_8px_rgb(0_0_0/0.8)]" style={i(0)}>
          {player ? `Welcome back, ${player}. ` : ""}
          {instance.lastPlayed ? `Last played ${ago(instance.lastPlayed).toLowerCase()}.` : "Ready for its first run."}
        </p>
        <button onClick={() => go({ name: "instance", id: instance.id })} className="group flex items-center gap-5 text-left">
          <span className="rise" style={i(1)}>
            <InstanceIcon instance={instance} size={88} shared tilt />
          </span>
          <div className="min-w-0">
            <h2
              className="rise truncate font-pixel text-[44px] font-medium leading-[1.05] tracking-tight decoration-gold decoration-2 underline-offset-[6px] [text-shadow:0_2px_16px_rgb(0_0_0/0.85)] group-hover:underline"
              style={i(2)}
            >
              {instance.name}
            </h2>
            <div className="rise mt-3 flex flex-wrap items-center gap-1.5" style={i(3)}>
              <span className="chip">Minecraft {instance.gameVersion}</span>
              <span className="chip">
                {loaderName[instance.loader]}
                {instance.loaderVersion ? ` ${instance.loaderVersion}` : ""}
              </span>
              <span className="chip">{playTime(instance.playTimeSecs)}</span>
            </div>
          </div>
        </button>
      </div>
      <div className="rise" style={i(3)}>
        <PlayButton instance={instance} size="hero" />
      </div>
    </section>
  );
}

function RecentTile({ instance, index }: { instance: Instance; index: number }) {
  const go = useStore((s) => s.go);
  return (
    <div className="card rise group flex items-center gap-3 rounded-[10px] p-3" style={i(index)}>
      <button onClick={() => go({ name: "instance", id: instance.id })} className="flex min-w-0 flex-1 items-center gap-3 text-left">
        <InstanceIcon instance={instance} size={44} shared tilt />
        <span className="min-w-0">
          <span className="block truncate font-medium">{instance.name}</span>
          <span className="block truncate text-[12px] text-ink-3">
            {loaderName[instance.loader]} {instance.gameVersion}
          </span>
        </span>
      </button>
      <PlayButton instance={instance} size="sm" />
    </div>
  );
}

function QuickAction({ icon, title, body, onClick, index }: { icon: React.ReactNode; title: string; body: string; onClick: () => void; index: number }) {
  return (
    <button onClick={onClick} className="card rise group flex items-center gap-3 rounded-[10px] px-4 py-3.5 text-left" style={i(index)}>
      <span className="grid h-9 w-9 place-items-center rounded-[8px] bg-slate-800 text-ink-2 transition-[transform,color,background] duration-200 group-hover:-rotate-6 group-hover:scale-110 group-hover:bg-gold/15 group-hover:text-gold">
        {icon}
      </span>
      <span>
        <span className="block font-medium">{title}</span>
        <span className="block text-[12px] text-ink-3">{body}</span>
      </span>
    </button>
  );
}

function FirstRun({ hasAccount }: { hasAccount: boolean }) {
  const setCreateOpen = useStore((s) => s.setCreateOpen);
  return (
    <section className="panel rise flex items-center gap-8 rounded-[14px] p-8">
      <Ori pose="hi" size={140} />
      <div className="max-w-lg">
        <h2 className="font-pixel text-[36px] font-medium leading-tight">Let's forge your first instance</h2>
        <p className="mt-3 text-ink-2">
          An instance is its own Minecraft folder with a version, a loader and its mods. Make as many as you like; they never touch each other.
          {!hasAccount && " Add an account from the bottom of the sidebar first so you can play."}
        </p>
        <button className="btn btn-gold mt-6 h-10 px-5" onClick={() => setCreateOpen(true)}>
          <Plus size={16} /> New instance
        </button>
      </div>
    </section>
  );
}
