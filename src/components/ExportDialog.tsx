// Exports an instance as a Modrinth .mrpack or a CurseForge .zip.

import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { motion } from "motion/react";
import { AlertTriangle, Check, FolderOpen, PackageOpen } from "lucide-react";
import { save as saveFile } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, isTauri } from "../lib/api";
import { bytes } from "../lib/format";
import { toastError } from "../lib/store";
import type { ExportFormat, ExportReport, Instance } from "../lib/types";
import { Dialog, Segmented } from "./ui";

export function ExportDialog({ instance, open, onClose }: { instance: Instance; open: boolean; onClose: () => void }) {
  const candidates = useQuery({ queryKey: ["exportCandidates", instance.id], queryFn: () => api.exportCandidates(instance.id), enabled: open });
  const [format, setFormat] = useState<ExportFormat>(instance.linkedPack?.source === "curseforge" ? "curseForge" : "mrpack");
  const [name, setName] = useState(instance.linkedPack?.name ?? instance.name);
  const [version, setVersion] = useState("1.0.0");
  const [author, setAuthor] = useState("");
  const [include, setInclude] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ExportReport | null>(null);

  useEffect(() => {
    if (candidates.data) setInclude(new Set(candidates.data.filter((c) => c.suggested).map((c) => c.path)));
  }, [candidates.data]);
  useEffect(() => {
    if (open) setReport(null);
  }, [open]);

  const ext = format === "mrpack" ? "mrpack" : "zip";
  const run = async () => {
    const fileName = `${name} ${version}`.replace(/[/\\:*?"<>|]/g, "").trim() + "." + ext;
    const dest = isTauri ? await saveFile({ defaultPath: fileName, filters: [{ name: format === "mrpack" ? "Modrinth modpack" : "CurseForge modpack", extensions: [ext] }] }) : `~/Downloads/${fileName}`;
    if (!dest) return;
    setBusy(true);
    try {
      setReport(await api.exportInstance(instance.id, { format, name: name.trim() || instance.name, version: version.trim() || "1.0.0", author: author.trim() || null, include: [...include], dest }));
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };
  const toggle = (p: string) =>
    setInclude((s) => {
      const n = new Set(s);
      if (n.has(p)) n.delete(p);
      else n.add(p);
      return n;
    });

  return (
    <Dialog open={open} onClose={onClose} title="Export as a modpack" width={560}>
      {report ? (
        <div className="flex flex-col gap-4 p-5">
          <div className="flex items-center gap-3">
            <span className="grid h-10 w-10 place-items-center rounded-full bg-grass/15 text-grass">
              <Check size={20} />
            </span>
            <div className="min-w-0">
              <p className="text-[15px] font-semibold text-ink">Exported, {bytes(report.size)}</p>
              <p className="truncate text-[12.5px] text-ink-3" data-selectable>{report.path}</p>
            </div>
          </div>
          <p className="text-[13.5px] text-ink-2">
            {report.linked} file{report.linked === 1 ? " is" : "s are"} listed by reference, so they download from {format === "mrpack" ? "Modrinth" : "CurseForge"} when the pack is installed.
          </p>
          {report.bundled.length > 0 && (
            <div className="flex gap-3 rounded-[8px] border border-molten/30 bg-molten/10 px-3 py-2.5">
              <AlertTriangle size={16} className="mt-0.5 shrink-0 text-molten" />
              <p className="text-[13px] text-ink-2">
                {format === "mrpack" ? "Modrinth" : "CurseForge"} doesn't know {report.bundled.length === 1 ? "this file" : `these ${report.bundled.length} files`}, so {report.bundled.length === 1 ? "it's" : "they're"} inside the pack:{" "}
                <span className="text-ink">{report.bundled.join(", ")}</span>. Check {report.bundled.length === 1 ? "its licence allows" : "their licences allow"} sharing before you publish the pack.
              </p>
            </div>
          )}
          <div className="flex justify-end gap-2 border-t border-line-soft pt-4">
            {isTauri && (
              <button className="btn btn-quiet" onClick={() => revealItemInDir(report.path).catch(toastError)}>
                <FolderOpen size={14} /> Show in folder
              </button>
            )}
            <button className="btn btn-gold" onClick={onClose}>
              Done
            </button>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-4 p-5">
          <Segmented
            label="Format"
            value={format}
            onChange={setFormat}
            options={[
              { value: "mrpack", label: "Modrinth (.mrpack)" },
              { value: "curseForge", label: "CurseForge (.zip)" },
            ]}
          />
          <div className="grid grid-cols-[1fr_120px] gap-3">
            <label>
              <span className="mb-1.5 block text-[12px] font-medium text-ink-2">Name</span>
              <input className="field" value={name} onChange={(e) => setName(e.target.value)} />
            </label>
            <label>
              <span className="mb-1.5 block text-[12px] font-medium text-ink-2">Version</span>
              <input className="field" value={version} onChange={(e) => setVersion(e.target.value)} />
            </label>
          </div>
          {format === "curseForge" && (
            <motion.label initial={{ opacity: 0, y: -4 }} animate={{ opacity: 1, y: 0 }}>
              <span className="mb-1.5 block text-[12px] font-medium text-ink-2">Author</span>
              <input className="field" value={author} onChange={(e) => setAuthor(e.target.value)} placeholder="Shown in the CurseForge App" />
            </motion.label>
          )}
          <div>
            <p className="mb-1.5 text-[12px] font-medium text-ink-2">Also put in the pack</p>
            {candidates.isLoading ? (
              <span className="spinner" />
            ) : !candidates.data?.length ? (
              <p className="text-[12.5px] text-ink-3">Nothing besides the mods, resource packs and shaders.</p>
            ) : (
              <ul className="flex max-h-48 flex-col gap-1 overflow-y-auto rounded-[8px] bg-slate-900/60 p-1.5">
                {candidates.data.map((c) => (
                  <li key={c.path}>
                    <label className="flex cursor-pointer items-center gap-3 rounded-[6px] px-2 py-1.5 transition-colors hover:bg-slate-800">
                      <input type="checkbox" className="accent-[var(--color-gold)]" checked={include.has(c.path)} onChange={() => toggle(c.path)} />
                      <span className="flex-1 truncate font-mono text-[12.5px] text-ink">{c.path}</span>
                      <span className="text-[12px] text-ink-3">
                        {c.files > 1 ? `${c.files} files, ` : ""}
                        {bytes(c.size)}
                      </span>
                    </label>
                  </li>
                ))}
              </ul>
            )}
            <p className="mt-1.5 text-[12px] text-ink-3">Mods, resource packs and shaders are always in. Worlds and your options are left out unless you tick them.</p>
          </div>
          <div className="flex justify-end gap-2 border-t border-line-soft pt-4">
            <button className="btn btn-quiet" onClick={onClose}>
              Cancel
            </button>
            <button className="btn btn-gold" onClick={run} disabled={busy || !name.trim()}>
              {busy ? <span className="spinner" /> : <PackageOpen size={14} />} Export
            </button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
