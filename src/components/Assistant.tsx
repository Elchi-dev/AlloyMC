// The assistant panel: a conversation with Claude beside whatever page is open. Claude looks
// at instances, logs and crashes with tools that run in Alloy, and asks before it changes
// anything.

import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { ArrowUp, Ban, Check, ChevronDown, History, LogIn, MessageSquarePlus, Sparkles, Square, Trash2, Wrench, X } from "lucide-react";
import { api } from "../lib/api";
import { useAssistant, userText, type Approval, type AssistantStatus, type Block, type DiffLine, type Msg } from "../lib/assistant";
import { openExternal } from "../lib/menus";
import { useInstance } from "../lib/queries";
import { ago } from "../lib/format";
import { toastError, useStore } from "../lib/store";
import { Segmented } from "./ui";
import { Ori } from "./ori/Ori";

const WIDTH = 420;

/** The rail button that opens and closes the panel. */
export function AssistantButton() {
  const open = useAssistant((s) => s.open);
  const busy = useAssistant((s) => s.busy);
  const waiting = useAssistant((s) => !!s.approval);
  return (
    <button
      onClick={() => useAssistant.getState().setOpen(!open)}
      aria-label="Assistant"
      aria-pressed={open}
      className={`group relative grid h-11 w-11 place-items-center rounded-[8px] transition-[color,background,transform] duration-150 active:scale-90 ${
        open ? "bg-slate-800 text-gold" : "text-ink-3 hover:bg-slate-850 hover:text-ink"
      }`}
    >
      <Sparkles size={19} strokeWidth={open ? 2.2 : 1.8} className={`relative transition-transform duration-200 group-hover:scale-110 ${busy ? "animate-pulse" : ""}`} />
      {waiting && !open && <span className="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-gold" />}
      <span className="pointer-events-none absolute left-[52px] z-50 -translate-x-1 whitespace-nowrap rounded-[6px] border border-line bg-slate-800 px-2 py-1 text-[12px] text-ink opacity-0 shadow-lg transition-[opacity,transform] duration-150 group-hover:translate-x-0 group-hover:opacity-100">
        Assistant
      </span>
    </button>
  );
}

/** Follows the page: on an instance page the conversation is about that instance. */
function useFollowPage() {
  const page = useStore((s) => s.page);
  useEffect(() => {
    const a = useAssistant.getState();
    if (a.messages.length > 0 || a.busy) return;
    a.setInstance(page.name === "instance" ? page.id : null);
  }, [page]);
}

export function AssistantPanel() {
  useFollowPage();
  const open = useAssistant((s) => s.open);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "j") {
        e.preventDefault();
        const a = useAssistant.getState();
        a.setOpen(!a.open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <AnimatePresence initial={false}>
      {open && (
        <motion.aside
          key="assistant"
          initial={{ width: 0, opacity: 0 }}
          animate={{ width: WIDTH, opacity: 1 }}
          exit={{ width: 0, opacity: 0 }}
          transition={{ duration: 0.24, ease: [0.22, 1, 0.36, 1] }}
          className="relative flex shrink-0 overflow-hidden border-l border-line-soft/60 bg-slate-900/92"
          aria-label="Assistant"
        >
          <div className="flex h-full flex-col" style={{ width: WIDTH }}>
            <Panel />
          </div>
        </motion.aside>
      )}
    </AnimatePresence>
  );
}

function Panel() {
  const [view, setView] = useState<"chat" | "history">("chat");
  const status = useQuery({ queryKey: ["assistant", "status"], queryFn: api.assistantStatus, staleTime: 30_000 });
  const busy = useAssistant((s) => s.busy);
  const qc = useQueryClient();
  // Usage moves with every reply.
  useEffect(() => {
    if (!busy) qc.invalidateQueries({ queryKey: ["assistant", "status"] });
  }, [busy, qc]);

  return (
    <>
      <Header view={view} setView={setView} />
      {view === "history" ? (
        <HistoryList onPick={() => setView("chat")} />
      ) : (
        <>
          <Conversation />
          <Footer status={status.data} loading={status.isLoading} />
        </>
      )}
    </>
  );
}

function Header({ view, setView }: { view: "chat" | "history"; setView: (v: "chat" | "history") => void }) {
  const busy = useAssistant((s) => s.busy);
  const hasMessages = useAssistant((s) => s.messages.length > 0);
  const instanceId = useAssistant((s) => s.instanceId);
  const inst = useInstance(instanceId ?? "");
  return (
    <div className="flex h-12 shrink-0 items-center gap-2 border-b border-line-soft/60 px-3">
      <Ori pose={busy ? "thinking" : "hi"} size={30} still={!busy} />
      <div className="min-w-0 flex-1">
        <p className="text-[13.5px] font-semibold leading-tight text-ink">Assistant</p>
        <p className="truncate text-[11.5px] leading-tight text-ink-3">{view === "history" ? "Earlier conversations" : inst.data ? `About ${inst.data.name}` : "Claude, inside Alloy"}</p>
      </div>
      <button className="btn btn-quiet h-8 w-8 p-0" aria-label="Earlier conversations" aria-pressed={view === "history"} onClick={() => setView(view === "history" ? "chat" : "history")}>
        <History size={15} />
      </button>
      <button
        className="btn btn-quiet h-8 w-8 p-0"
        aria-label="New conversation"
        disabled={!hasMessages && view === "chat"}
        onClick={() => {
          useAssistant.getState().reset();
          setView("chat");
        }}
      >
        <MessageSquarePlus size={15} />
      </button>
      <button className="btn btn-quiet h-8 w-8 p-0" aria-label="Close the assistant" onClick={() => useAssistant.getState().setOpen(false)}>
        <X size={15} />
      </button>
    </div>
  );
}

// ---------- conversation ----------

function Conversation() {
  const messages = useAssistant((s) => s.messages);
  const live = useAssistant((s) => s.live);
  const busy = useAssistant((s) => s.busy);
  const error = useAssistant((s) => s.error);
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  // Results by tool use id, to show how each tool call went.
  const results = useMemo(() => {
    const m = new Map<string, Extract<Block, { type: "tool_result" }>>();
    for (const msg of messages) for (const b of msg.content) if (b.type === "tool_result") m.set(b.tool_use_id, b);
    return m;
  }, [messages]);

  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  });

  return (
    <div
      ref={ref}
      className="min-h-0 flex-1 overflow-y-auto px-4 py-4"
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
      }}
    >
      {messages.length === 0 && !busy ? (
        <Welcome />
      ) : (
        <div className="flex flex-col gap-3">
          {messages.map((m, i) => (
            <MessageView key={i} m={m} results={results} />
          ))}
          {live && <LiveReply blocks={live.blocks} />}
          {error && <p className="rounded-[8px] border border-redstone/30 bg-redstone/10 px-3 py-2 text-[13px] text-ink-2">{error}</p>}
        </div>
      )}
    </div>
  );
}

const ideas = [
  "Why does my game crash?",
  "Build me a cozy 1.21.1 pack with Create and shaders",
  "Make this instance run faster",
  "What does each mod in this pack do?",
];

function Welcome() {
  const send = useAssistant((s) => s.send);
  return (
    <div className="flex flex-col items-center pt-6 text-center">
      <Ori pose="hi" size={110} />
      <h3 className="mt-3 text-[16px] font-semibold text-ink">Ask about your Minecraft</h3>
      <p className="mt-1 max-w-[300px] text-[13px] text-ink-3">
        Claude can look at your instances, logs and crashes, find mods, and fix things. It asks before it changes anything.
      </p>
      <div className="mt-5 flex w-full flex-col gap-2">
        {ideas.map((t) => (
          <button key={t} className="rounded-[8px] border border-line-soft bg-slate-850/70 px-3 py-2 text-left text-[13px] text-ink-2 transition-colors hover:border-line hover:text-ink" onClick={() => void send(t)}>
            {t}
          </button>
        ))}
      </div>
    </div>
  );
}

function MessageView({ m, results }: { m: Msg; results: Map<string, Extract<Block, { type: "tool_result" }>> }) {
  if (m.role === "user") {
    const text = userText(m);
    if (!text) return null;
    return (
      <div className="ml-10 self-end whitespace-pre-wrap rounded-[12px] rounded-br-[4px] bg-slate-800 px-3 py-2 text-[13.5px] text-ink" data-selectable>
        {text}
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      {m.content.map((b, i) =>
        b.type === "text" ? <Markdown key={i} text={b.text} /> : b.type === "tool_use" ? <ToolRow key={i} id={b.id} name={b.name} result={results.get(b.id)} /> : null,
      )}
    </div>
  );
}

function LiveReply({ blocks }: { blocks: Record<number, { text?: string; tool?: string }> }) {
  const keys = Object.keys(blocks).map(Number).sort((a, b) => a - b);
  if (keys.length === 0) return <Working />;
  return (
    <div className="flex flex-col gap-2">
      {keys.map((k) =>
        blocks[k].tool ? (
          <ToolRow key={k} id={`live-${k}`} name={blocks[k].tool!} pending />
        ) : (
          <Markdown key={k} text={blocks[k].text ?? ""} />
        ),
      )}
    </div>
  );
}

function Working() {
  return (
    <div className="flex items-center gap-2 text-[12.5px] text-ink-3">
      <span className="spinner" /> Thinking
    </div>
  );
}

const verbs: Record<string, string> = {
  list_instances: "Looking at your instances",
  get_instance: "Checking the instance",
  search_projects: "Searching",
  get_project: "Reading a project page",
  read_log: "Reading the log",
  read_crash: "Reading the crash",
  read_file: "Reading a file",
  create_instance: "Getting a new instance ready",
  install_content: "Getting an install ready",
  set_enabled: "Getting a change ready",
  remove_content: "Getting a change ready",
  apply_fix: "Getting a fix ready",
  update_content: "Checking for updates",
  write_file: "Writing a config change",
  set_memory: "Getting a change ready",
  test_launch: "Getting a test start ready",
};

function ToolRow({ id, name, result, pending }: { id: string; name: string; result?: Extract<Block, { type: "tool_result" }>; pending?: boolean }) {
  const label = useAssistant((s) => s.labels[id]);
  const running = useAssistant((s) => s.running === id);
  const waiting = useAssistant((s) => s.approval?.toolUseId === id);
  const [open, setOpen] = useState(false);
  const declined = result?.is_error && /declined|Stopped by the player/.test(result.content);
  const failed = result?.is_error && !declined;
  const icon = pending || running || !result ? <span className="spinner" /> : declined ? <Ban size={13} className="text-ink-3" /> : failed ? <X size={13} className="text-redstone" /> : <Check size={13} className="text-grass" />;
  return (
    <div className="text-[12.5px]">
      <button
        className="flex w-full items-center gap-2 rounded-[6px] px-1 py-0.5 text-left text-ink-3 transition-colors hover:text-ink-2 disabled:hover:text-ink-3"
        onClick={() => setOpen(!open)}
        disabled={!result}
        aria-expanded={open}
      >
        <span className="grid w-4 shrink-0 place-items-center">{icon}</span>
        <span className="min-w-0 flex-1 truncate">{label ?? verbs[name] ?? name}</span>
        {waiting && <span className="shrink-0 text-[11.5px] font-medium text-gold">Waiting for you</span>}
        {result && <ChevronDown size={13} className={`shrink-0 transition-transform ${open ? "rotate-180" : ""}`} />}
      </button>
      <AnimatePresence initial={false}>
        {open && result && (
          <motion.pre
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="mt-1 max-h-60 overflow-auto whitespace-pre-wrap rounded-[6px] bg-slate-950/80 px-2.5 py-2 font-mono text-[11px] leading-relaxed text-ink-3"
            data-selectable
          >
            {result.content}
          </motion.pre>
        )}
      </AnimatePresence>
    </div>
  );
}

function Markdown({ text }: { text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const html = useMemo(() => DOMPurify.sanitize(marked.parse(text, { async: false, breaks: true }) as string), [text]);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onClick = (e: MouseEvent) => {
      const a = (e.target as HTMLElement).closest("a");
      if (a?.href) {
        e.preventDefault();
        void openExternal(a.href);
      }
    };
    el.addEventListener("click", onClick);
    return () => el.removeEventListener("click", onClick);
  }, []);
  if (!text.trim()) return null;
  return <div ref={ref} className="prose-alloy prose-chat" data-selectable dangerouslySetInnerHTML={{ __html: html }} />;
}

// ---------- approval, input and status ----------

function Footer({ status, loading }: { status?: AssistantStatus; loading: boolean }) {
  const approval = useAssistant((s) => s.approval);
  return (
    <div className="shrink-0 border-t border-line-soft/60 px-3 pb-3 pt-2.5">
      <AnimatePresence initial={false}>{approval && <ApprovalCard key={approval.toolUseId} a={approval} />}</AnimatePresence>
      {loading ? null : !status?.signedIn ? (
        <SignInNotice />
      ) : !status.available ? (
        <Unavailable status={status} />
      ) : (
        <Composer status={status} />
      )}
    </div>
  );
}

function Notice({ text, action }: { text: string; action?: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2 rounded-[10px] border border-line-soft bg-slate-850/70 p-3 text-[13px] text-ink-2">
      <p>{text}</p>
      {action}
    </div>
  );
}

function SignInNotice() {
  const [busy, setBusy] = useState(false);
  return (
    <Notice
      text="The assistant comes with your free AlloyMC account. Sign in, confirm your email and link a Minecraft account to use it."
      action={
        <button
          className="btn btn-gold self-start"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await api.alloymcSignIn();
            } catch (e) {
              toastError(e);
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? <span className="spinner" /> : <LogIn size={14} />} Sign in to AlloyMC
        </button>
      }
    />
  );
}

function Unavailable({ status }: { status: AssistantStatus }) {
  if (status.needs === "email")
    return <Notice text={status.reason ?? "Confirm your email first."} action={<button className="btn self-start" onClick={() => openExternal("https://alloymc.dev/settings")}>Open alloymc.dev</button>} />;
  const resets = status.dayResets ? new Date(status.dayResets) : null;
  const text = status.reason ?? "The assistant isn't available right now.";
  const back = resets && status.dayUsed >= 100 ? ` It's back at ${resets.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}.` : "";
  return <Notice text={text + back} />;
}

function Meter({ status }: { status: AssistantStatus }) {
  if (status.unlimited) return null;
  const used = Math.max(status.dayUsed, status.monthUsed);
  const which = status.dayUsed >= status.monthUsed ? "today's" : "this month's";
  return (
    <div className="flex items-center gap-2 text-[11px] text-ink-3" title={`Today ${status.dayUsed}% · this month ${status.monthUsed}%`}>
      <div className="h-1 w-16 overflow-hidden rounded-full bg-slate-700">
        <div className={`h-full rounded-full ${used > 85 ? "bg-molten" : "bg-gold"}`} style={{ width: `${Math.min(100, used)}%` }} />
      </div>
      {used}% of {which} use
    </div>
  );
}

function Composer({ status }: { status: AssistantStatus }) {
  const [text, setText] = useState("");
  const busy = useAssistant((s) => s.busy);
  const mode = useAssistant((s) => s.mode);
  const pending = useAssistant((s) => !!s.approval);
  const ref = useRef<HTMLTextAreaElement>(null);
  const open = useAssistant((s) => s.open);
  useEffect(() => {
    if (open) ref.current?.focus();
  }, [open]);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(160, el.scrollHeight)}px`;
  }, [text]);
  const submit = () => {
    if (!text.trim() || busy) return;
    void useAssistant.getState().send(text);
    setText("");
  };
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-end gap-2 rounded-[10px] border border-line bg-slate-950/70 p-1.5 pl-3 focus-within:border-gold/60">
        <textarea
          ref={ref}
          rows={1}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            }
          }}
          placeholder={pending ? "Allow or decline the change above" : busy ? "Claude is working…" : "Ask about mods, crashes, packs…"}
          disabled={busy}
          maxLength={8000}
          className="max-h-40 min-h-[30px] flex-1 resize-none bg-transparent py-1 text-[13.5px] text-ink outline-none placeholder:text-ink-3"
          aria-label="Message"
        />
        {busy ? (
          <button className="btn h-8 w-8 shrink-0 p-0" aria-label="Stop" onClick={() => useAssistant.getState().stop()}>
            <Square size={12} fill="currentColor" />
          </button>
        ) : (
          <button className="btn btn-gold h-8 w-8 shrink-0 p-0" aria-label="Send" disabled={!text.trim()} onClick={submit}>
            <ArrowUp size={15} />
          </button>
        )}
      </div>
      <div className="flex items-center justify-between">
        <Segmented
          label="How hard Claude thinks"
          value={mode}
          onChange={(m) => useAssistant.getState().setMode(m)}
          options={[
            { value: "fast", label: "Quick" },
            { value: "deep", label: "Think harder" },
          ]}
        />
        <Meter status={status} />
      </div>
    </div>
  );
}

function ApprovalCard({ a }: { a: Approval }) {
  const answer = useAssistant((s) => s.answer);
  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 8, transition: { duration: 0.12 } }}
      className="mb-2.5 rounded-[10px] border border-gold/40 bg-slate-850 p-3 shadow-lg"
      role="alertdialog"
      aria-label="Allow this change?"
    >
      <div className="flex items-start gap-2">
        <Wrench size={14} className="mt-0.5 shrink-0 text-gold" />
        <div className="min-w-0 flex-1">
          <p className="text-[13.5px] font-medium text-ink">{a.title}</p>
          {a.detail && <p className="mt-0.5 max-h-28 overflow-y-auto whitespace-pre-wrap text-[12.5px] text-ink-3">{a.detail}</p>}
        </div>
      </div>
      {a.diff && <Diff lines={a.diff} />}
      <div className="mt-3 flex flex-wrap items-center justify-end gap-2">
        {!a.always && (
          <button className="mr-auto text-[12px] text-ink-3 transition-colors hover:text-ink" onClick={() => answer(true, true)}>
            Allow everything in this chat
          </button>
        )}
        <button className="btn btn-quiet h-8" onClick={() => answer(false)}>
          Decline
        </button>
        <button className="btn btn-gold h-8" onClick={() => answer(true)} autoFocus>
          <Check size={13} /> Allow
        </button>
      </div>
    </motion.div>
  );
}

function Diff({ lines }: { lines: DiffLine[] }) {
  return (
    <pre className="mt-2 max-h-56 overflow-auto rounded-[6px] bg-slate-950/80 py-1.5 font-mono text-[11px] leading-[1.55]" data-selectable>
      {lines.map((l, i) =>
        l.kind === "gap" ? (
          <div key={i} className="px-2.5 text-ink-3/70">
            ⋯ {l.count} unchanged line{l.count > 1 ? "s" : ""}
          </div>
        ) : (
          <div key={i} className={`whitespace-pre-wrap px-2.5 ${l.kind === "add" ? "bg-grass/10 text-grass" : l.kind === "del" ? "bg-redstone/10 text-redstone" : "text-ink-3"}`}>
            {l.kind === "add" ? "+ " : l.kind === "del" ? "- " : "  "}
            {l.text || " "}
          </div>
        ),
      )}
    </pre>
  );
}

// ---------- history ----------

function HistoryList({ onPick }: { onPick: () => void }) {
  const { data, refetch } = useQuery({ queryKey: ["assistant", "chats"], queryFn: api.assistantChats });
  const current = useAssistant((s) => s.chatId);
  const chats = data ?? [];
  if (chats.length === 0)
    return (
      <div className="flex flex-1 flex-col items-center justify-center px-6 text-center">
        <Ori pose="sleeping" size={90} />
        <p className="mt-3 text-[13px] text-ink-3">No conversations yet. They're kept on this computer.</p>
      </div>
    );
  return (
    <ul className="min-h-0 flex-1 overflow-y-auto p-2">
      {chats.map((c) => (
        <li key={c.id} className="group flex items-center gap-1">
          <button
            className={`min-w-0 flex-1 rounded-[8px] px-3 py-2 text-left transition-colors hover:bg-slate-800 ${c.id === current ? "bg-slate-800/70" : ""}`}
            onClick={async () => {
              try {
                await useAssistant.getState().load(c.id);
                onPick();
              } catch (e) {
                toastError(e);
              }
            }}
          >
            <p className="truncate text-[13px] text-ink">{c.title}</p>
            <p className="text-[11.5px] text-ink-3">{ago(c.updated)}</p>
          </button>
          <button
            className="btn btn-quiet h-8 w-8 shrink-0 p-0 opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100"
            aria-label={`Delete “${c.title}”`}
            onClick={async () => {
              await api.assistantChatDelete(c.id).catch(toastError);
              if (c.id === current) useAssistant.getState().reset();
              void refetch();
            }}
          >
            <Trash2 size={13} />
          </button>
        </li>
      ))}
    </ul>
  );
}

