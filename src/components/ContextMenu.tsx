// A right-click menu at the pointer, kept inside the window, with optional submenus.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion } from "motion/react";
import { ChevronRight } from "lucide-react";

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
