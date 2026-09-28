import { api } from "../../lib/api";
import { formatCount } from "../../lib/format";
import type { AppStatus, IndexProgress } from "../../lib/types";

const pct = (p: IndexProgress) => (p.total > 0 ? Math.min(100, Math.floor((p.done / p.total) * 100)) : null);

/** One line per phase. Once messages are indexed, it says word search already works. */
export function progressText(p: IndexProgress): string {
  const n = pct(p);
  switch (p.phase) {
    case "reading":
      return "Reading your messages";
    case "indexing":
      return p.total > 0 ? `Indexing messages ${formatCount(p.done)} of ${formatCount(p.total)}` : `Indexing messages ${formatCount(p.done)}`;
    case "windows":
      return "Preparing meaning search. Word search works now.";
    case "downloadingModel":
      return `Downloading the meaning model${n !== null ? ` ${n}%` : ""}. Word search works now.`;
    case "embedding":
      return `Meaning search ${n !== null ? `${n}%` : "building"}. Word search works now.`;
    case "idle":
      return "";
  }
}

/**
 * The quiet status area: indexing progress (overall, never backward), the
 * last indexing error, and a hint when Contacts access is off.
 */
export function StatusLine({ status, progress, bar = true }: { status: AppStatus; progress: IndexProgress | null; bar?: boolean }) {
  const busy = progress && progress.phase !== "idle";
  const frac = busy && progress.total > 0 ? Math.min(1, progress.done / progress.total) : null;
  if (!busy && !status.error && status.contacts !== "denied") return null;
  return (
    <div className="status" role="status" aria-live="polite">
      {busy && (
        <div className="status-row">
          <span className="status-dot" />
          <span className="status-text">{progressText(progress)}</span>
          {bar && frac !== null && (
            <span className="status-track">
              <span className="status-fill" style={{ width: `${(frac * 100).toFixed(1)}%` }} />
            </span>
          )}
        </div>
      )}
      {!busy && status.error && (
        <div className="status-row is-error" title={status.error}>
          <span className="status-dot" />
          <span className="status-text">{status.error}</span>
        </div>
      )}
      {status.contacts === "denied" && (
        <div className="status-row is-muted">
          <span className="status-text">Names and photos need Contacts access.</span>
          <button className="text-btn" onClick={() => void api.openContactsSettings()}>
            Open Settings
          </button>
        </div>
      )}
    </div>
  );
}
