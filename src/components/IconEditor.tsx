// Choosing an instance icon: any image (cropped to a square, pixel art kept sharp) or one
// of the block textures. Images can also be dropped straight onto the dialog.

import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ImagePlus, RotateCcw } from "lucide-react";
import { api } from "../lib/api";
import { queryClient } from "../lib/queries";
import { toastError, useStore } from "../lib/store";
import type { Instance } from "../lib/types";
import { BlockTile, InstanceIcon } from "./Art";
import { Dialog, Segmented } from "./ui";

const VIEW = 240; // crop viewport, px
const OUT = 256; // saved icon, px

export function IconEditor({ instance, open, onClose, initialFile, initialSrc }: { instance: Instance; open: boolean; onClose: () => void; initialFile?: File | null; initialSrc?: string | null }) {
  const textures = useStore((s) => s.textures);
  const [tab, setTab] = useState<"image" | "block">("image");
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const [zoom, setZoom] = useState(1);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [busy, setBusy] = useState(false);
  const [over, setOver] = useState(false);
  const drag = useRef<{ x: number; y: number; px: number; py: number } | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const preview = useRef<HTMLCanvasElement>(null);

  const loadSrc = (src: string) => {
    const i = new Image();
    i.onload = () => {
      setImg(i);
      setZoom(1);
      setPos({ x: 0, y: 0 });
      setTab("image");
    };
    i.onerror = () => toastError("That image couldn't be read");
    i.src = src;
  };
  const load = (file: File) => {
    if (!file.type.startsWith("image/")) return toastError("That isn't an image");
    const reader = new FileReader();
    reader.onload = () => loadSrc(String(reader.result));
    reader.readAsDataURL(file);
  };
  useEffect(() => {
    if (open && initialFile) load(initialFile);
    else if (open && initialSrc) loadSrc(initialSrc);
    if (!open) setTimeout(() => setImg(null), 200);
  }, [open, initialFile, initialSrc]);

  // Base scale covers the square; zoom multiplies it.
  const base = img ? VIEW / Math.min(img.width, img.height) : 1;
  const scale = base * zoom;
  const pixelArt = !!img && Math.max(img.width, img.height) <= 128;

  const clamp = (p: { x: number; y: number }) => {
    if (!img) return p;
    const w = (img.width * scale - VIEW) / 2;
    const h = (img.height * scale - VIEW) / 2;
    return { x: Math.max(-w, Math.min(w, p.x)), y: Math.max(-h, Math.min(h, p.y)) };
  };

  const render = (canvas: HTMLCanvasElement, size: number) => {
    if (!img) return;
    const ctx = canvas.getContext("2d")!;
    canvas.width = canvas.height = size;
    ctx.imageSmoothingEnabled = !pixelArt;
    ctx.imageSmoothingQuality = "high";
    const k = size / VIEW;
    const w = img.width * scale * k;
    const h = img.height * scale * k;
    ctx.clearRect(0, 0, size, size);
    ctx.drawImage(img, size / 2 - w / 2 + pos.x * k, size / 2 - h / 2 + pos.y * k, w, h);
  };
  useEffect(() => {
    if (preview.current) render(preview.current, 64);
  });

  const save = async () => {
    setBusy(true);
    try {
      const c = document.createElement("canvas");
      render(c, OUT);
      await api.setInstanceIcon(instance.id, { kind: "image", data: c.toDataURL("image/png") });
      await queryClient.invalidateQueries({ queryKey: ["instanceIcon", instance.id] });
      onClose();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };
  const pickBlock = async (name: string) => {
    try {
      await api.setInstanceIcon(instance.id, { kind: "block", name });
      onClose();
    } catch (e) {
      toastError(e);
    }
  };
  const reset = async () => {
    await api.setInstanceIcon(instance.id, { kind: "none" }).catch(toastError);
    onClose();
  };

  return (
    <Dialog open={open} onClose={onClose} title="Instance icon" width={600}>
      <div
        className="relative flex flex-col gap-4 p-5"
        onDragOver={(e) => (e.preventDefault(), setOver(true))}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          const f = e.dataTransfer.files[0];
          if (f) load(f);
        }}
      >
        <div className="flex items-center gap-3">
          <Segmented label="Icon source" value={tab} onChange={setTab} options={[{ value: "image", label: "Image" }, { value: "block", label: "Block" }]} />
          <button className="btn btn-quiet ml-auto" onClick={reset}>
            <RotateCcw size={14} /> Automatic
          </button>
        </div>

        <AnimatePresence mode="wait">
          {tab === "image" ? (
            <motion.div key="image" initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} className="flex items-start gap-5">
              <div
                className="relative shrink-0 cursor-grab overflow-hidden rounded-[12px] bg-slate-950 active:cursor-grabbing"
                style={{ width: VIEW, height: VIEW, backgroundImage: "conic-gradient(#1a1c22 25%, #22252c 0 50%, #1a1c22 0 75%, #22252c 0)", backgroundSize: "16px 16px" }}
                onPointerDown={(e) => {
                  if (!img) return;
                  (e.target as Element).setPointerCapture(e.pointerId);
                  drag.current = { x: e.clientX, y: e.clientY, px: pos.x, py: pos.y };
                }}
                onPointerMove={(e) => {
                  const d = drag.current;
                  if (d) setPos(clamp({ x: d.px + e.clientX - d.x, y: d.py + e.clientY - d.y }));
                }}
                onPointerUp={() => (drag.current = null)}
                onWheel={(e) => img && setZoom((z) => Math.min(6, Math.max(1, z * (e.deltaY < 0 ? 1.08 : 0.93))))}
              >
                {img ? (
                  <img
                    src={img.src}
                    alt=""
                    draggable={false}
                    className={`pointer-events-none absolute left-1/2 top-1/2 max-w-none ${pixelArt ? "pixelated" : ""}`}
                    style={{ width: img.width * scale, height: img.height * scale, transform: `translate(calc(-50% + ${pos.x}px), calc(-50% + ${pos.y}px))` }}
                  />
                ) : (
                  <button className="absolute inset-0 grid place-items-center text-ink-3 transition-colors hover:text-ink" onClick={() => fileInput.current?.click()}>
                    <span className="flex flex-col items-center gap-2 text-[13px]">
                      <ImagePlus size={28} />
                      Choose or drop an image
                    </span>
                  </button>
                )}
                <div className="pointer-events-none absolute inset-0 rounded-[12px] ring-1 ring-inset ring-white/10" />
              </div>
              <div className="flex min-w-0 flex-1 flex-col gap-4">
                <div className="flex items-end gap-3">
                  <canvas ref={preview} className={`h-16 w-16 rounded-[8px] bg-slate-800 ${img ? "" : "opacity-0"}`} />
                  <span className={`h-8 w-8 overflow-hidden rounded-[6px] ${img ? "" : "opacity-0"}`}>
                    <canvas ref={(c) => { if (c && img) render(c, 32); }} className="h-8 w-8" />
                  </span>
                  <span className="pb-1 text-[12px] text-ink-3">{img ? "How it looks in the library and the sidebar" : ""}</span>
                </div>
                <label className={`flex flex-col gap-1.5 text-[12px] text-ink-2 ${img ? "" : "pointer-events-none opacity-40"}`}>
                  Zoom
                  <input type="range" min={1} max={6} step={0.01} value={zoom} onChange={(e) => (setZoom(Number(e.target.value)), setPos((p) => clamp(p)))} className="accent-[var(--color-gold)]" />
                </label>
                {pixelArt && <p className="text-[12px] text-ink-3">Small image: kept pixel-sharp instead of blurred.</p>}
                <div className="mt-auto flex gap-2">
                  <button className="btn" onClick={() => fileInput.current?.click()}>
                    {img ? "Another image" : "Choose image"}
                  </button>
                  <button className="btn btn-gold ml-auto" disabled={!img || busy} onClick={save}>
                    {busy ? <span className="spinner" /> : null} Use this icon
                  </button>
                </div>
              </div>
            </motion.div>
          ) : (
            <motion.div key="block" initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} className="grid max-h-[300px] grid-cols-[repeat(auto-fill,minmax(52px,1fr))] gap-2 overflow-y-auto">
              {Object.keys(textures)
                .filter((n) => n !== "lava_still")
                .sort()
                .map((name, i) => (
                  <button key={name} onClick={() => pickBlock(name)} className="rise grid place-items-center rounded-[8px] p-1.5 transition-[background,transform] hover:-translate-y-0.5 hover:bg-slate-800 active:scale-95" style={{ ["--i" as string]: Math.min(i, 20) } as React.CSSProperties} title={name.replace(/_/g, " ")}>
                    <BlockTile name={name} size={42} />
                  </button>
                ))}
              {!Object.keys(textures).length && <p className="col-span-full text-[13px] text-ink-3">Block textures haven't been downloaded yet.</p>}
            </motion.div>
          )}
        </AnimatePresence>

        <input ref={fileInput} type="file" accept="image/png,image/jpeg,image/webp,image/gif" className="hidden" onChange={(e) => e.target.files?.[0] && load(e.target.files[0])} />
        {over && (
          <div className="drop-frame pointer-events-none absolute inset-2 grid place-items-center rounded-[10px] bg-slate-950/70 text-[14px] font-medium">
            <span className="flex items-center gap-2">
              <InstanceIcon instance={instance} size={24} /> Drop to use as the icon
            </span>
          </div>
        )}
      </div>
    </Dialog>
  );
}
