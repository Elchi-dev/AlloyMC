import { useMemo, useState } from "react";
import { Plus, Search } from "lucide-react";
import { InstanceIcon } from "../components/Art";
import { PlayButton } from "../components/PlayButton";
import { Empty, Segmented } from "../components/ui";
import { ago, loaderName, progressFraction, progressLabel } from "../lib/format";
import { useInstances } from "../lib/queries";
import { useStore } from "../lib/store";
import type { Instance } from "../lib/types";

type Sort = "played" | "name" | "created";

export function LibraryPage() {
  const { data } = useInstances();
  const setCreateOpen = useStore((s) => s.setCreateOpen);
  const [q, setQ] = useState("");
  const [sort, setSort] = useState<Sort>("played");

  const list = useMemo(() => {
    const term = q.trim().toLowerCase();
    const filtered = (data ?? []).filter(
      (i) => !term || i.name.toLowerCase().includes(term) || i.gameVersion.includes(term) || i.loader.includes(term),
    );
    return filtered.sort((a, b) =>
      sort === "name"
        ? a.name.localeCompare(b.name)
        : sort === "created"
          ? b.createdAt - a.createdAt
          : (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0),
    );
  }, [data, q, sort]);

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
          body="Create one from scratch, install a modpack from Discover, or import a .mrpack or CurseForge zip."
          pose="hi"
          action={<button className="btn btn-gold" onClick={() => setCreateOpen(true)}>New instance</button>}
        />
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-3">
          {list.map((inst, n) => (
            <Tile key={inst.id} instance={inst} index={n} />
          ))}
        </div>
      )}
      {data && data.length > 0 && !list.length && <p className="fade-in text-ink-3">Nothing matches “{q}”.</p>}
    </div>
  );
}

function Tile({ instance, index }: { instance: Instance; index: number }) {
  const go = useStore((s) => s.go);
  const progress = useStore((s) => s.progress[instance.id]);
  const running = useStore((s) => s.game[instance.id] === "running");
  return (
    <div className="card rise group relative flex flex-col rounded-[12px]" style={{ ["--i" as string]: index } as React.CSSProperties}>
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
