import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, onIndexChanged, onIndexProgress } from "./lib/api";
import { parseQuery, termsRegex } from "./lib/query";
import { formatCount } from "./lib/format";
import { groupHits, visibleHits } from "./lib/group";
import { helpSeen, markHelpSeen } from "./lib/prefs";
import type { AppStatus, ChatSummary, IndexProgress, SearchHit } from "./lib/types";
import { Icon } from "./components/Icon";
import { Setup } from "./features/setup/Setup";
import { Home } from "./features/search/Home";
import { SearchPill } from "./features/search/SearchPill";
import { FilterPill } from "./features/search/FilterPill";
import { Results } from "./features/search/Results";
import { StatusLine } from "./features/search/StatusLine";
import { SEARCH_LIMIT, useSearch } from "./features/search/useSearch";
import { ConversationPanel } from "./features/conversation/ConversationPanel";
import { HelpModal } from "./features/help/HelpModal";
import { SettingsModal } from "./features/settings/SettingsModal";
import { InsightsPage } from "./features/insights/InsightsPage";
import { StoragePage } from "./features/storage/StoragePage";
import { NavPill, type View } from "./components/NavPill";
import { LightboxHost } from "./components/Lightbox";
import { Toasts } from "./components/Toasts";

export default function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [progress, setProgress] = useState<IndexProgress | null>(null);
  /** Bumps on `index-changed`: re-run the search, refresh the open conversation. */
  const [version, setVersion] = useState(0);

  useEffect(() => {
    const refresh = () => api.status().then(setStatus, () => {});
    api.status().then((s) => {
      setStatus(s);
      setProgress(s.progress);
    });
    const subs = [
      onIndexProgress((p) => {
        // A finished run: pick up the final stats and any indexing error.
        if (p.phase === "idle") refresh();
        setProgress((prev) => {
          if (p.phase === "idle") return null;
          // Overall numbers only move forward; ignore a stale event that would step back.
          if (prev && prev.phase === p.phase && prev.total === p.total && p.done < prev.done) return prev;
          return p;
        });
      }),
      onIndexChanged(() => {
        setVersion((v) => v + 1);
        refresh();
      }),
    ];
    // Permissions can change in System Settings while we're in the background.
    window.addEventListener("focus", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
      subs.forEach((s) => s.then((un) => un()));
    };
  }, []);

  const onStatus = useCallback((s: AppStatus) => {
    setStatus(s);
    setProgress(s.progress);
  }, []);

  if (!status) return <div className="app-loading" data-tauri-drag-region />;
  if (status.access !== "ok") return <Setup status={status} onStatus={onStatus} />;
  return (
    <>
      <Main status={status} progress={progress} version={version} />
      <LightboxHost />
      <Toasts />
    </>
  );
}

type ModalKind = "help" | "settings" | null;

function Main({ status, progress, version }: { status: AppStatus; progress: IndexProgress | null; version: number }) {
  // `query` is the committed search (Enter, a filter, an example); `draft` is what's in the box.
  const initial = new URLSearchParams(location.search).get("q") ?? "";
  const [query, setQuery] = useState(initial);
  const [draft, setDraft] = useState(initial);
  const { results, error, pending } = useSearch(query, version);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [openHit, setOpenHit] = useState<SearchHit | null>(null);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [modal, setModal] = useState<ModalKind>(() => (helpSeen() ? null : "help"));
  const [view, setView] = useState<View>("search");
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const transcriptRef = useRef<HTMLDivElement>(null);

  // The chat list only feeds the Person/Conversation menus: fetch it when one
  // opens, and keep it until new messages arrive (index-changed bumps `version`).
  const [chats, setChats] = useState<ChatSummary[]>([]);
  const chatsFor = useRef(-1);
  const loadChats = useCallback(() => {
    if (chatsFor.current === version) return;
    chatsFor.current = version;
    api.listChats(2000).then(setChats, () => (chatsFor.current = -1));
  }, [version]);

  const toggleGroup = useCallback(
    (key: string) =>
      setExpanded((s) => {
        const n = new Set(s);
        if (n.has(key)) n.delete(key);
        else n.add(key);
        return n;
      }),
    [],
  );
  const selectHit = useCallback((h: SearchHit) => setSelectedId(h.messageId), []);

  const hits = useMemo(() => results?.hits ?? [], [results]);
  const groups = useMemo(() => groupHits(hits), [hits]);
  const visible = useMemo(() => visibleHits(groups, expanded), [groups, expanded]);
  const parsed = useMemo(() => parseQuery(query), [query]);
  const highlight = useMemo(() => termsRegex(parsed), [parsed]);
  const onResults = query.trim() !== "";

  /** Run a search. A new query closes the conversation and starts at the top. */
  const commit = useCallback((q: string) => {
    setView("search");
    setDraft(q);
    setQuery(q);
    setOpenHit(null);
    setSelectedId(null);
    setExpanded(new Set());
    listRef.current?.scrollTo({ top: 0 });
  }, []);

  const goHome = useCallback(() => {
    commit("");
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [commit]);

  // Switching between home and results swaps the search box: keep typing focus in it.
  useEffect(() => {
    const el = inputRef.current;
    if (!el || modal) return;
    el.focus({ preventScroll: true });
    el.setSelectionRange(el.value.length, el.value.length);
  }, [onResults, view]);

  // New results: select the top hit unless the selection is still there.
  useEffect(() => {
    if (!results) return;
    setSelectedId((cur) => (cur != null && results.hits.some((h) => h.messageId === cur) ? cur : results.hits[0]?.messageId ?? null));
  }, [results]);

  const open = useCallback((h: SearchHit) => {
    setSelectedId(h.messageId);
    setOpenHit(h);
  }, []);

  const move = useCallback(
    (delta: number) => {
      if (!visible.length) return;
      const i = visible.findIndex((h) => h.messageId === selectedId);
      const next = visible[Math.max(0, Math.min(visible.length - 1, (i < 0 ? -1 : i) + delta))];
      setSelectedId(next.messageId);
      // With a conversation open, it follows the selection (like Mail).
      if (openHit) setOpenHit(next);
    },
    [visible, selectedId, openHit],
  );

  const closePanel = useCallback(() => {
    setOpenHit(null);
    listRef.current?.focus({ preventScroll: true });
  }, []);

  const closeModal = useCallback(() => {
    if (modal === "help") markHelpSeen();
    setModal(null);
  }, [modal]);

  // Global shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.ctrlKey || e.altKey) return;
      const k = e.key.toLowerCase();
      if (k === "k" || k === "f") {
        e.preventDefault();
        if (modal) closeModal();
        const el = inputRef.current;
        el?.focus();
        el?.setSelectionRange(el.value.length, el.value.length);
      } else if (k === "/" || k === "?") {
        e.preventDefault();
        if (modal === "help") closeModal();
        else setModal("help");
      } else if (k === ",") {
        e.preventDefault();
        setModal((m) => (m === "settings" ? null : "settings"));
      } else if (k === "1" || k === "2" || k === "3") {
        e.preventDefault();
        setView(k === "1" ? "search" : k === "2" ? "insights" : "storage");
      } else if (k === "o" && openHit?.chatId != null) {
        e.preventDefault();
        void api.openInMessages(openHit.chatId);
      } else if (k === "[") {
        e.preventDefault();
        if (openHit) closePanel();
        else if (onResults) goHome();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [modal, openHit, onResults, closeModal, closePanel, goHome]);

  // Results page keys (bubble up from the search box and the list).
  const onResultsKey = (e: React.KeyboardEvent) => {
    if (e.metaKey || e.ctrlKey || e.altKey || modal) return;
    const inInput = e.target === inputRef.current;
    const selected = visible.find((h) => h.messageId === selectedId) ?? null;
    switch (e.key) {
      case "ArrowDown":
        move(1);
        break;
      case "ArrowUp":
        move(-1);
        break;
      case "PageDown":
        move(6);
        break;
      case "PageUp":
        move(-6);
        break;
      case "Enter":
        // Changed text searches (the form handles it); otherwise Enter opens the selection.
        if (inInput && draft.trim() !== query.trim()) return;
        if (selected) open(selected);
        break;
      case "ArrowRight":
        if (inInput && inputRef.current!.selectionStart !== draft.length) return;
        if (!openHit && selected) open(selected);
        transcriptRef.current?.focus({ preventScroll: true });
        break;
      case "Escape":
        if (openHit) closePanel();
        else if (inInput && draft !== query) setDraft(query);
        else goHome();
        break;
      default:
        return;
    }
    e.preventDefault();
  };

  const modalEl =
    modal === "help" ? (
      <HelpModal
        onClose={closeModal}
        onRun={(q) => {
          markHelpSeen();
          setModal(null);
          commit(q);
        }}
        onSettings={() => {
          markHelpSeen();
          setModal("settings");
        }}
      />
    ) : modal === "settings" ? (
      <SettingsModal initial={status} onClose={closeModal} onHelp={() => setModal("help")} />
    ) : null;

  const nav = <NavPill view={view} onView={setView} />;
  const tools = (
    <div className="topbar-tools">
      <button className="round-btn" onClick={() => setModal("help")} aria-label="Help" title="Help (⌘/)">
        <Icon name="help" size={18} />
      </button>
      <button className="round-btn" onClick={() => setModal("settings")} aria-label="Settings" title="Settings (⌘,)">
        <Icon name="gear" size={18} />
      </button>
    </div>
  );

  // The section segment and help/settings sit in fixed window positions and
  // are identical on every page, so switching pages never moves them.
  const chrome = (
    <div className="app-chrome">
      <div className="app-chrome-nav">{nav}</div>
      <div className="app-chrome-tools">{tools}</div>
    </div>
  );

  if (view !== "search") {
    return (
      <div className="app-page">
        <header className="topbar" data-tauri-drag-region />
        {view === "insights" ? <InsightsPage onSearch={commit} onStorage={() => setView("storage")} /> : <StoragePage />}
        {chrome}
        {modalEl}
      </div>
    );
  }

  if (!onResults) {
    return (
      <>
        <Home
          draft={draft}
          onDraft={setDraft}
          onSubmit={() => commit(draft.trim())}
          onQuery={commit}
          inputRef={inputRef}
          chats={chats}
          onNeedChats={loadChats}
          status={status}
          progress={progress}
          onHelp={() => setModal("help")}
        />
        {chrome}
        {modalEl}
      </>
    );
  }

  const meaning = hits.filter((h) => h.matchedBy === "meaning").length;
  const summary = error ? (
    <span className="results-line is-error">{error}</span>
  ) : results ? (
    <span className={`results-line${pending ? " is-pending" : ""}`}>
      {hits.length === 0
        ? "No results"
        : `${formatCount(groups.length)} ${groups.length === 1 ? "conversation" : "conversations"} · ${formatCount(hits.length)}${
            hits.length >= SEARCH_LIMIT ? "+" : ""
          } ${hits.length === 1 ? "message" : "messages"}${meaning === hits.length ? ", all by meaning" : meaning ? `, ${meaning} by meaning` : ""}`}
      {" · "}
      {results.tookMs < 1 ? "<1" : Math.round(results.tookMs)} ms
    </span>
  ) : (
    <span className="results-line">Searching</span>
  );
  const header = (
    <div className="results-head">
      <StatusLine status={status} progress={progress} bar={false} />
    </div>
  );

  const hasFilters = parsed.from.length + parsed.chat.length + parsed.has.length > 0 || parsed.afterMs !== null || parsed.beforeMs !== null;

  return (
    <div className={`results-page${openHit ? " has-panel" : ""}`} onKeyDown={onResultsKey}>
      <header className="topbar is-results" data-tauri-drag-region>
        <button className="round-btn tb-back" onClick={goHome} aria-label="Back to start" title="Back (Esc)">
          <Icon name="back" size={18} />
        </button>
        <div className="topbar-search tb-search" data-tauri-drag-region>
          {/* Enter searches here; the attached button lives on home only. */}
          <SearchPill size="bar" submit={false} value={draft} onChange={setDraft} onSubmit={() => commit(draft.trim())} inputRef={inputRef} />
        </div>
        <div className="topbar-sub tb-sub" data-tauri-drag-region>
          <FilterPill query={draft} onQuery={commit} chats={chats} onNeedChats={loadChats} stats={status.stats} />
          {summary}
        </div>
      </header>
      <div className="results-body">
        {results && hits.length === 0 && !pending ? (
          <div className="results-scroll">
            <div className="results-col">
              {header}
              <NoResults
                hasFilters={hasFilters}
                onClearFilters={() => {
                  const q = parsed.words.concat(parsed.phrases.map((p) => `"${p}"`)).join(" ");
                  commit(q);
                }}
              />
            </div>
          </div>
        ) : (
          <div className="results-list" ref={listRef} tabIndex={-1}>
            <Results
              groups={groups}
              expanded={expanded}
              onToggle={toggleGroup}
              selectedId={selectedId}
              openId={openHit?.messageId ?? null}
              onSelect={selectHit}
              onOpen={open}
              scrollRef={listRef}
              header={header}
              markSimilar={meaning > 0 && meaning < hits.length}
            />
          </div>
        )}
        {openHit && (
          <ConversationPanel
            key={openHit.chatId ?? openHit.messageId}
            hit={openHit}
            highlight={highlight}
            version={version}
            scrollerRef={transcriptRef}
            onClose={closePanel}
            onEscape={() => listRef.current?.focus({ preventScroll: true })}
          />
        )}
      </div>
      {chrome}
      {modalEl}
    </div>
  );
}

function NoResults({ hasFilters, onClearFilters }: { hasFilters: boolean; onClearFilters: () => void }) {
  return (
    <div className="empty-state">
      <div className="empty-state-col">
        <div className="empty-art">
          <div className="empty-tile">
            <Icon name="search" size={26} />
          </div>
        </div>
        <h2>No messages found</h2>
        <p>Try fewer words, check the spelling, or describe what it was about and meaning search will look for it.</p>
        {hasFilters && (
          <div className="empty-actions">
            <button className="btn" onClick={onClearFilters}>
              Remove Filters
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
