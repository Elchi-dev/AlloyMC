// A right-click menu at the pointer, kept inside the window, with optional submenus.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion } from "motion/react";
import { ChevronRight, ClipboardPaste, Copy, ExternalLink, Scissors, TextSelect } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "../lib/api";
import { useStore } from "../lib/store";

export interface MenuItem {
  label: string;
  icon?: React.ComponentType<{ size?: number }>;
  run?: () => void;
  danger?: boolean;
  disabled?: boolean;
  children?: MenuItem[];
  separator?: boolean;
}

export function ContextMenu({ at, items, onClose }: { at: { x: number; y: number } | null; items: MenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState(at);
  const [sub, setSub] = useState<number | null>(null);

  useLayoutEffect(() => {
    if (!at || !ref.current) return setPos(at);
    const r = ref.current.getBoundingClientRect();
    setPos({ x: Math.min(at.x, window.innerWidth - r.width - 8), y: Math.min(at.y, window.innerHeight - r.height - 8) });
  }, [at]);
  useEffect(() => {
    if (!at) return;
    setSub(null);
    const close = (e: Event) => !ref.current?.contains(e.target as Node) && onClose();
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
      window.removeEventListener("blur", onClose);
    };
  }, [at, onClose]);

  return createPortal(
    <AnimatePresence>
      {at && (
        <motion.div
          ref={ref}
          role="menu"
          initial={{ opacity: 0, scale: 0.96 }}
          animate={{ opacity: 1, scale: 1 }}
          exit={{ opacity: 0, scale: 0.97, transition: { duration: 0.1 } }}
          transition={{ duration: 0.12 }}
          className="fixed z-[80] w-52 origin-top-left rounded-[10px] border border-line bg-slate-850 p-1 shadow-2xl shadow-black/50"
          style={{ left: pos?.x ?? at.x, top: pos?.y ?? at.y }}
          onContextMenu={(e) => e.preventDefault()}
        >
          {items.map((item, i) =>
            item.separator ? (
              <div key={`sep-${i}`} className="mx-2 my-1 h-px bg-line-soft" />
            ) : (
              <div key={item.label} className="relative" onMouseEnter={() => setSub(item.children ? i : null)}>
                <button
                  role="menuitem"
                  disabled={item.disabled}
                  onClick={() => {
                    if (item.children) return setSub(i);
                    item.run?.();
                    onClose();
                  }}
                  className={`flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-slate-800 disabled:opacity-40 ${item.danger ? "text-redstone" : ""}`}
                >
                  {item.icon && <item.icon size={14} />}
                  <span className="flex-1 truncate">{item.label}</span>
                  {item.children && <ChevronRight size={13} className="text-ink-3" />}
                </button>
                <AnimatePresence>
                  {sub === i && item.children && (
                    <motion.div
                      initial={{ opacity: 0, x: -4 }}
                      animate={{ opacity: 1, x: 0 }}
                      exit={{ opacity: 0 }}
                      transition={{ duration: 0.1 }}
                      className="absolute left-full top-0 ml-1 w-48 rounded-[10px] border border-line bg-slate-850 p-1 shadow-2xl shadow-black/50"
                    >
                      {item.children.map((c) => (
                        <button
                          key={c.label}
                          role="menuitem"
                          disabled={c.disabled}
                          onClick={() => {
                            c.run?.();
                            onClose();
                          }}
                          className="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left text-[13px] transition-colors hover:bg-slate-800 disabled:opacity-40"
                        >
                          {c.icon && <c.icon size={14} />}
                          <span className="truncate">{c.label}</span>
                        </button>
                      ))}
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            ),
          )}
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}

/** The one right-click menu for the whole window. Components open it with
 *  `useStore.getState().openMenu(e, items)`; anywhere else, text fields get cut, copy and
 *  paste, selected text gets copy, links get open and copy, and nothing else gets the
 *  browser's own menu (Reload and Inspect don't belong in an app). */
export function GlobalMenu() {
  const menu = useStore((s) => s.menu);
  const close = useStore((s) => s.closeMenu);
  useEffect(() => {
    const onMenu = (e: MouseEvent) => {
      if (e.defaultPrevented) return;
      e.preventDefault();
      const items = defaultItems(e.target as HTMLElement);
      if (items.length) useStore.getState().openMenu(e, items);
    };
    document.addEventListener("contextmenu", onMenu);
    return () => document.removeEventListener("contextmenu", onMenu);
  }, []);
  return <ContextMenu at={menu?.at ?? null} items={menu?.items ?? []} onClose={close} />;
}

const copyText = (t: string) => navigator.clipboard.writeText(t).catch(() => {});

function defaultItems(target: HTMLElement): MenuItem[] {
  const field = target.closest("input, textarea, [contenteditable='true']") as HTMLInputElement | HTMLTextAreaElement | HTMLElement | null;
  const selected = window.getSelection()?.toString() ?? "";
  if (field && !(field instanceof HTMLInputElement && ["checkbox", "radio", "range", "button", "submit"].includes(field.type))) {
    const editable = !(field as HTMLInputElement).readOnly && !(field as HTMLInputElement).disabled;
    const sel = "selectionStart" in field && field.selectionStart !== field.selectionEnd;
    const keep = () => field.focus();
    return [
      { label: "Cut", icon: Scissors, disabled: !editable || !sel, run: () => (keep(), document.execCommand("cut")) },
      { label: "Copy", icon: Copy, disabled: !sel && !selected, run: () => (keep(), document.execCommand("copy")) },
      {
        label: "Paste",
        icon: ClipboardPaste,
        disabled: !editable,
        run: () =>
          navigator.clipboard
            .readText()
            .then((t) => (keep(), document.execCommand("insertText", false, t)))
            .catch(() => {}),
      },
      { separator: true, label: "" },
      { label: "Select all", icon: TextSelect, run: () => (keep(), (field as HTMLInputElement).select?.() ?? document.execCommand("selectAll")) },
    ];
  }
  const link = target.closest("a[href]") as HTMLAnchorElement | null;
  const items: MenuItem[] = [];
  if (selected.trim()) items.push({ label: "Copy", icon: Copy, run: () => copyText(selected) });
  if (link && /^https?:/.test(link.href)) {
    items.push(
      { label: "Open link", icon: ExternalLink, run: () => (isTauri ? openUrl(link.href) : window.open(link.href, "_blank")) },
      { label: "Copy link", icon: Copy, run: () => copyText(link.href) },
    );
  }
  return items;
}
