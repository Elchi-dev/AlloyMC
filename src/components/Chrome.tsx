// Window chrome: the custom title bar and the left rail.

import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Check, Compass, ExternalLink, Home, LayoutGrid, Link2, LogIn, LogOut, Minus, Plus, Settings, Square, Trash2, UserRound, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, isTauri } from "../lib/api";
import { useAccounts, useAlloyMc } from "../lib/queries";
import type { AlloyProfile } from "../lib/types";
import { toastError, useStore, type Page } from "../lib/store";
import { PlayerHead } from "./Art";
import { Logo } from "./Logo";
import { DownloadsButton } from "./Downloads";
import { UpdateButton } from "./Updates";
import { AssistantButton } from "./Assistant";

export function TitleBar({ title }: { title: string }) {
  const win = isTauri ? getCurrentWindow() : null;
  return (
    <header data-tauri-drag-region className="drag flex h-11 shrink-0 items-center border-b border-line-soft/60 pl-6">
      <AnimatePresence mode="wait" initial={false}>
        <motion.h1
          key={title}
          data-tauri-drag-region
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -4 }}
          transition={{ duration: 0.16 }}
          className="text-[13px] font-semibold text-ink-2"
        >
          {title}
        </motion.h1>
      </AnimatePresence>
      <div className="no-drag ml-auto flex h-full">
        {[
          { label: "Minimize", icon: <Minus size={14} />, run: () => win?.minimize() },
          { label: "Maximize", icon: <Square size={11} />, run: () => win?.toggleMaximize() },
          { label: "Close", icon: <X size={15} />, run: () => win?.close(), danger: true },
        ].map((b) => (
          <button
            key={b.label}
            aria-label={b.label}
            onClick={b.run}
            className={`grid h-full w-11 place-items-center text-ink-3 transition-colors duration-150 hover:bg-slate-800 hover:text-ink active:scale-90 ${b.danger ? "hover:!bg-redstone hover:!text-white" : ""}`}
          >
            {b.icon}
          </button>
        ))}
      </div>
    </header>
  );
}

const nav: { page: Page["name"]; label: string; icon: typeof Home }[] = [
  { page: "home", label: "Home", icon: Home },
  { page: "library", label: "Library", icon: LayoutGrid },
  { page: "discover", label: "Discover", icon: Compass },
];

export function Rail() {
  const page = useStore((s) => s.page);
  const go = useStore((s) => s.go);
  const setCreateOpen = useStore((s) => s.setCreateOpen);

  const item = (name: Page["name"], label: string, Icon: typeof Home, onClick?: () => void, badge?: number) => {
    const on = !onClick && (page.name === name || (name === "library" && page.name === "instance") || (name === "discover" && page.name === "project"));
    return (
      <button
        key={label}
        onClick={onClick ?? (() => go({ name } as Page))}
        aria-label={label}
        aria-current={on ? "page" : undefined}
        className={`group relative grid h-11 w-11 place-items-center rounded-[8px] transition-[color,background,transform] duration-150 active:scale-90 ${
          on ? "text-ink" : "text-ink-3 hover:bg-slate-850 hover:text-ink"
        }`}
      >
        {on && (
          <>
            <motion.span layoutId="rail-bg" className="absolute inset-0 rounded-[8px] bg-slate-800" transition={{ type: "spring", stiffness: 500, damping: 38 }} />
            <motion.span layoutId="rail-mark" className="absolute -left-[10px] h-5 w-[3px] rounded-r bg-gold" transition={{ type: "spring", stiffness: 500, damping: 38 }} />
          </>
        )}
        <Icon size={19} strokeWidth={on ? 2.2 : 1.8} className="relative transition-transform duration-200 group-hover:scale-110" />
        <AnimatePresence>
          {!!badge && (
            <motion.span
              initial={{ scale: 0 }}
              animate={{ scale: 1 }}
              exit={{ scale: 0 }}
              transition={{ type: "spring", stiffness: 600, damping: 22 }}
              className="absolute right-1 top-1 grid h-4 min-w-4 place-items-center rounded-full bg-gold px-1 text-[10px] font-bold text-gold-ink"
            >
              {badge}
            </motion.span>
          )}
        </AnimatePresence>
        <span className="pointer-events-none absolute left-[52px] z-50 -translate-x-1 whitespace-nowrap rounded-[6px] border border-line bg-slate-800 px-2 py-1 text-[12px] text-ink opacity-0 shadow-lg transition-[opacity,transform] duration-150 group-hover:translate-x-0 group-hover:opacity-100">
          {label}
        </span>
      </button>
    );
  };

  return (
    <nav data-tauri-drag-region className="drag flex w-16 shrink-0 flex-col items-center gap-1 border-r border-line-soft/60 bg-slate-900/85 py-3">
      <button className="no-drag mb-2 grid h-11 w-11 place-items-center" onClick={() => go({ name: "home" })} aria-label="Alloy home">
        <Logo size={34} intro />
      </button>
      <div className="no-drag flex flex-col items-center gap-1">
        {nav.map((n) => item(n.page, n.label, n.icon))}
        <div className="my-2 h-px w-7 bg-line" />
        {item("home", "New instance", Plus, () => setCreateOpen(true))}
        <AssistantButton />
      </div>
      <div className="no-drag mt-auto flex flex-col items-center gap-1">
        <UpdateButton />
        <DownloadsButton />
        {item("settings", "Settings", Settings)}
        <AccountMenu />
      </div>
    </nav>
  );
}

function AccountMenu() {
  const { data } = useAccounts();
  const [open, setOpen] = useState(false);
  const [offlineName, setOfflineName] = useState("");
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const active = data?.accounts.find((a) => a.id === data.active);
  const { profile } = useAlloyMc();
  const [linking, setLinking] = useState<string | null>(null);
  const linked = new Set(profile?.minecraft.map((m) => m.uuid.replace(/-/g, "")) ?? []);

  const link = async (id: string, name: string) => {
    setLinking(id);
    try {
      await api.alloymcLink(id);
      useStore.getState().toast({ kind: "success", text: `${name} is on your AlloyMC profile now` });
    } catch (e) {
      toastError(e);
    } finally {
      setLinking(null);
    }
  };

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);

  const signIn = async () => {
    setBusy(true);
    try {
      const a = await api.loginMicrosoft();
      useStore.getState().toast({ kind: "success", text: `Signed in as ${a.username}` });
    } catch (e) {
      if (!String(e).includes("cancelled")) toastError(e);
    } finally {
      setBusy(false);
    }
  };

  const addOffline = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      await api.addOffline(offlineName);
      setOfflineName("");
    } catch (err) {
      toastError(err);
    }
  };

  return (
    <div ref={ref} className="relative mt-1">
      <button
        onClick={() => setOpen((v) => !v)}
        aria-label={active ? `Account: ${active.username}` : "Add an account"}
        aria-expanded={open}
        className="grid h-11 w-11 place-items-center rounded-[8px] transition-transform duration-150 hover:bg-slate-850 active:scale-90"
      >
        {active ? <PlayerHead account={active} size={30} /> : <UserRound size={19} className="text-ink-3" />}
        {active?.needsLogin && <span className="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-redstone" />}
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, x: -6, scale: 0.98 }}
            animate={{ opacity: 1, x: 0, scale: 1 }}
            exit={{ opacity: 0, x: -6, scale: 0.98 }}
            transition={{ duration: 0.14 }}
            className="absolute bottom-0 left-[56px] z-50 w-72 origin-bottom-left rounded-[10px] border border-line bg-slate-850 p-1.5 shadow-2xl shadow-black/50"
          >
            <AlloyMcCard profile={profile ?? null} />
            <div className="mx-2 my-1.5 h-px bg-line-soft" />
            {data?.accounts.map((a) => (
              <div key={a.id} className="group flex items-center rounded-[7px] hover:bg-slate-800">
                <button
                  onClick={() => api.setActiveAccount(a.id).catch(toastError)}
                  className="flex min-w-0 flex-1 items-center gap-3 px-2 py-2 text-left"
                >
                  <PlayerHead account={a} size={28} />
                  <span className="min-w-0">
                    <span className="block truncate font-medium">{a.username}</span>
                    <span className={`block text-[12px] ${a.needsLogin ? "text-redstone" : "text-ink-3"}`}>
                      {a.needsLogin ? "Sign in again" : a.kind === "microsoft" ? "Microsoft" : "Offline"}
                    </span>
                  </span>
                  {a.id === data.active && <span className="ml-auto h-2 w-2 shrink-0 rounded-full bg-gold" aria-label="Active" />}
                </button>
                {profile && a.kind === "microsoft" && !a.needsLogin && (
                  linked.has(a.id) ? (
                    <span className="mr-1 grid h-7 w-7 place-items-center text-grass" title="On your AlloyMC profile" aria-label={`${a.username} is linked to AlloyMC`}>
                      <Check size={14} />
                    </span>
                  ) : (
                    <button
                      onClick={() => link(a.id, a.username)}
                      disabled={linking === a.id}
                      aria-label={`Link ${a.username} to your AlloyMC profile`}
                      title="Show this account on your AlloyMC profile"
                      className="mr-1 grid h-7 place-items-center rounded-[6px] px-1.5 text-[12px] text-ink-3 transition-colors hover:bg-slate-700 hover:text-gold disabled:opacity-60"
                    >
                      {linking === a.id ? <span className="spinner h-3.5 w-3.5" /> : <span className="flex items-center gap-1"><Link2 size={13} /> Link</span>}
                    </button>
                  )
                )}
                <button
                  onClick={() => api.removeAccount(a.id).catch(toastError)}
                  aria-label={`Remove ${a.username}`}
                  className="mr-1 grid h-7 w-7 place-items-center rounded-[6px] text-ink-3 opacity-0 hover:bg-slate-700 hover:text-redstone group-hover:opacity-100 focus:opacity-100"
                >
                  <Trash2 size={14} />
                </button>
              </div>
            ))}
            {!!data?.accounts.length && <div className="mx-2 my-1.5 h-px bg-line-soft" />}
            <button onClick={signIn} disabled={busy} className="btn btn-gold w-full justify-center">
              <LogIn size={15} /> {busy ? "Waiting for Microsoft…" : "Sign in with Microsoft"}
            </button>
            <form onSubmit={addOffline} className="mt-1.5 flex gap-1.5">
              <input
                value={offlineName}
                onChange={(e) => setOfflineName(e.target.value)}
                placeholder="Offline player name"
                maxLength={16}
                className="field h-8"
                aria-label="Offline player name"
              />
              <button className="btn h-8" disabled={offlineName.length < 3}>
                Add
              </button>
            </form>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

const openExternal = (url: string) => (isTauri ? openUrl(url) : Promise.resolve(window.open(url, "_blank"))).catch(toastError);

/** The AlloyMC account at the top of the account menu. */
function AlloyMcCard({ profile }: { profile: AlloyProfile | null }) {
  const [waiting, setWaiting] = useState(false);

  const signIn = async () => {
    setWaiting(true);
    try {
      const p = await api.alloymcSignIn();
      useStore.getState().toast({ kind: "success", text: `Signed in to AlloyMC as @${p.handle}` });
    } catch (e) {
      if (!String(e).includes("cancelled")) toastError(e);
    } finally {
      setWaiting(false);
    }
  };

  if (!profile) {
    return (
      <div className="rounded-[8px] border border-line-soft bg-slate-900/60 p-3">
        <div className="flex items-center gap-2">
          <Logo size={20} />
          <span className="font-medium">AlloyMC account</span>
          <span className="ml-auto text-[11px] text-ink-3">Optional</span>
        </div>
        <p className="mt-1.5 text-[12px] leading-snug text-ink-3">A profile at alloymc.dev with your skin and the people you follow. Alloy works the same without it.</p>
        {waiting ? (
          <div className="mt-2.5 flex items-center gap-2">
            <span className="spinner h-4 w-4" />
            <span className="flex-1 text-[12px] text-ink-2">Finish in your browser…</span>
            <button className="btn h-7 px-2.5 text-[12px]" onClick={() => api.alloymcCancelSignIn()}>Cancel</button>
          </div>
        ) : (
          <button onClick={signIn} className="btn mt-2.5 w-full justify-center">
            <LogIn size={15} /> Sign in to AlloyMC
          </button>
        )}
      </div>
    );
  }
  return (
    <div className="rounded-[8px] border border-line-soft bg-slate-900/60 p-2.5">
      <div className="flex items-center gap-2.5">
        <img src={`${profile.avatarUrl}${profile.avatarUrl.includes("?") ? "&" : "?"}s=96`} alt="" className="h-9 w-9 rounded-[7px] bg-slate-800 [image-rendering:pixelated]" />
        <span className="min-w-0 flex-1">
          <span className="block truncate font-medium">{profile.displayName}</span>
          <span className="block truncate text-[12px] text-ink-3">
            @{profile.handle} · {profile.followers} {profile.followers === 1 ? "follower" : "followers"}
          </span>
        </span>
        <button
          onClick={() => openExternal(profile.profileUrl)}
          aria-label="Open your profile on alloymc.dev"
          title="Open your profile"
          className="grid h-7 w-7 place-items-center rounded-[6px] text-ink-3 transition-colors hover:bg-slate-700 hover:text-ink"
        >
          <ExternalLink size={14} />
        </button>
        <button
          onClick={() => api.alloymcSignOut().catch(toastError)}
          aria-label="Sign out of AlloyMC"
          title="Sign out of AlloyMC"
          className="grid h-7 w-7 place-items-center rounded-[6px] text-ink-3 transition-colors hover:bg-slate-700 hover:text-redstone"
        >
          <LogOut size={14} />
        </button>
      </div>
    </div>
  );
}
