import { HomeBackground } from "../../components/HomeBackground";
import { formatCount } from "../../lib/format";
import type { AppStatus, ChatSummary, IndexProgress } from "../../lib/types";
import { FilterPill } from "./FilterPill";
import { SearchPill } from "./SearchPill";
import { StatusLine } from "./StatusLine";

const EXAMPLES = ["gate code", "where should we eat in austin", "from:me has:link", "during:2024 sunset", '"see you soon"'];

function coverage(status: AppStatus): string | null {
  const { messages, oldestMs, newestMs } = status.stats;
  if (!messages) return null;
  const parts = [`${formatCount(messages)} messages`];
  if (oldestMs) {
    const from = new Date(oldestMs).getFullYear();
    const recent = newestMs && Date.now() - newestMs < 45 * 86_400_000;
    const to = recent ? "today" : newestMs ? String(new Date(newestMs).getFullYear()) : "today";
    parts.push(`${from} to ${to}`);
  }
  return parts.join(" · ");
}

/** The one-page start: headline, a big pill search, the filter pill, a few examples. */
export function Home({
  draft,
  onDraft,
  onSubmit,
  onQuery,
  inputRef,
  chats,
  onNeedChats,
  status,
  progress,
  onHelp,
}: {
  draft: string;
  onDraft: (v: string) => void;
  onSubmit: () => void;
  /** Run this exact query (examples, filter picks). */
  onQuery: (q: string) => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
  chats: ChatSummary[];
  onNeedChats: () => void;
  status: AppStatus;
  progress: IndexProgress | null;
  onHelp: () => void;
}) {
  const sub = coverage(status);
  return (
    <main className="home">
      <HomeBackground />
      <div className="titlebar" data-tauri-drag-region />
      <div className="home-center">
        <h1 className="home-title">Search every message</h1>
        <p className="home-sub">{sub ?? " "}</p>
        <SearchPill
          size="hero"
          value={draft}
          onChange={onDraft}
          onSubmit={onSubmit}
          inputRef={inputRef}
          autoFocus
          onKeyDown={(e) => {
            if (e.key === "Escape" && draft) {
              e.preventDefault();
              onDraft("");
            }
          }}
        />
        <FilterPill query={draft} onQuery={onQuery} chats={chats} onNeedChats={onNeedChats} stats={status.stats} />
        <div className="home-examples">
          {EXAMPLES.map((q) => (
            <button key={q} className="home-example" onClick={() => onQuery(q)}>
              {q}
            </button>
          ))}
          <button className="home-example is-more" onClick={onHelp}>
            More ways to search
          </button>
        </div>
      </div>
      <div className="home-status">
        <StatusLine status={status} progress={progress} />
      </div>
    </main>
  );
}
