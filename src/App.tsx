import { lazy, Suspense, useEffect } from "react";
import { AnimatePresence, LayoutGroup, motion } from "motion/react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Backdrop } from "./components/Backdrop";
import { Rail, TitleBar } from "./components/Chrome";
import { CreateInstanceDialog } from "./components/CreateInstance";
import { Toasts } from "./components/ui";
import { useUpdates } from "./components/Updates";
import { api, isTauri } from "./lib/api";
import { listen } from "./lib/bus";
import { useBackendEvents, useInstance, useSettings } from "./lib/queries";
import { useStore, type Page } from "./lib/store";
import { HomePage } from "./pages/Home";
import { LibraryPage } from "./pages/Library";

// Less-visited pages load on demand, then are prefetched once the app is idle.
const loadInstance = () => import("./pages/Instance");
const loadDiscover = () => import("./pages/Discover");
const loadSettings = () => import("./pages/Settings");
const InstancePage = lazy(() => loadInstance().then((m) => ({ default: m.InstancePage })));
const DiscoverPage = lazy(() => loadDiscover().then((m) => ({ default: m.DiscoverPage })));
const SettingsPage = lazy(() => loadSettings().then((m) => ({ default: m.SettingsPage })));

function useTitle(page: Page) {
  const inst = useInstance(page.name === "instance" ? page.id : "");
  switch (page.name) {
    case "home": return "Home";
    case "library": return "Library";
    case "discover": return "Discover";
    case "settings": return "Settings";
    case "instance": return inst.data?.name ?? "Instance";
  }
}

/** Motion level from settings; loops pause while a game runs or Alloy is in the background. */
function useAmbient() {
  const { data: settings } = useSettings();
  const running = useStore((s) => Object.values(s.game).some((g) => g === "running"));
  useEffect(() => {
    document.documentElement.dataset.motion = settings?.motion ?? "full";
  }, [settings?.motion]);
  useEffect(() => {
    const root = document.documentElement;
    const update = () => {
      if (running || document.hidden || !document.hasFocus()) root.dataset.paused = "";
      else delete root.dataset.paused;
    };
    update();
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    document.addEventListener("visibilitychange", update);
    return () => {
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      document.removeEventListener("visibilitychange", update);
    };
  }, [running]);
}

function useTextures() {
  useEffect(() => {
    const load = () => api.textures().then(useStore.getState().setTextures).catch(() => {});
    load();
    return listen("textures-ready", load);
  }, []);
}

/** Frameless windows have no resize border on Linux; these thin edges provide one. */
function ResizeEdges() {
  if (!isTauri) return null;
  const win = getCurrentWindow();
  const edges: [string, string][] = [
    ["North", "top-0 left-2 right-2 h-1 cursor-n-resize"],
    ["South", "bottom-0 left-2 right-2 h-1 cursor-s-resize"],
    ["West", "left-0 top-2 bottom-2 w-1 cursor-w-resize"],
    ["East", "right-0 top-2 bottom-2 w-1 cursor-e-resize"],
    ["NorthWest", "left-0 top-0 h-2 w-2 cursor-nw-resize"],
    ["NorthEast", "right-0 top-0 h-2 w-2 cursor-ne-resize"],
    ["SouthWest", "left-0 bottom-0 h-2 w-2 cursor-sw-resize"],
    ["SouthEast", "right-0 bottom-0 h-2 w-2 cursor-se-resize"],
  ];
  return (
    <>
      {edges.map(([dir, cls]) => (
        <div
          key={dir}
          className={`no-drag fixed z-[60] ${cls}`}
          onPointerDown={(e) => {
            if (e.button === 0) win.startResizeDragging(dir as never).catch(() => {});
          }}
        />
      ))}
    </>
  );
}

export default function App() {
  useBackendEvents();
  useAmbient();
  useTextures();
  useUpdates();
  const page = useStore((s) => s.page);
  const title = useTitle(page);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        useStore.getState().setCreateOpen(true);
      }
      if (e.altKey && e.key === "ArrowLeft") useStore.getState().back();
    };
    window.addEventListener("keydown", onKey);
    // Show the window once the first frame is painted (no white flash), then prefetch pages.
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (isTauri) getCurrentWindow().show().catch(() => {});
        const idle = (window as any).requestIdleCallback ?? ((f: () => void) => setTimeout(f, 300));
        idle(() => {
          loadInstance();
          loadDiscover();
          loadSettings();
        });
      }),
    );
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const key = page.name === "instance" ? `instance-${page.id}` : page.name;

  return (
    <div className="flex h-full">
      <Backdrop />
      <ResizeEdges />
      <Rail />
      <div className="flex min-w-0 flex-1 flex-col">
        <TitleBar title={title} />
        <main className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden">
          <LayoutGroup>
            <AnimatePresence mode="popLayout" initial={false}>
              <motion.div
                key={key}
                className="h-full"
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0, transition: { duration: 0.3, ease: [0.22, 1, 0.36, 1] } }}
                exit={{ opacity: 0, y: -6, transition: { duration: 0.14, ease: [0.55, 0, 0.75, 0.1] } }}
              >
                <Suspense fallback={null}>
                  {page.name === "home" && <HomePage />}
                  {page.name === "library" && <LibraryPage />}
                  {page.name === "discover" && <DiscoverPage target={page.target} kind={page.kind} />}
                  {page.name === "instance" && <InstancePage id={page.id} tab={page.tab} />}
                  {page.name === "settings" && <SettingsPage />}
                </Suspense>
              </motion.div>
            </AnimatePresence>
          </LayoutGroup>
        </main>
      </div>
      <CreateInstanceDialog />
      <Toasts />
    </div>
  );
}
