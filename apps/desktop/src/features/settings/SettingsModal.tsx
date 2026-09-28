import { useEffect, useState } from "react";
import { Modal, Row, Section } from "../../components/Modal";
import { api, asCommandError, openLink } from "../../lib/api";
import { formatBytes, formatCount } from "../../lib/format";
import { HIGHLIGHT_STYLES, setHighlightStyle, useHighlightStyle } from "../../lib/prefs";
import type { AppStatus, ContactsAccess } from "../../lib/types";

const POLL_MS = 2000;
const monthYear = new Intl.DateTimeFormat(undefined, { month: "short", year: "numeric" });

const CONTACTS: Record<ContactsAccess, string> = {
  authorized: "Allowed",
  denied: "Not allowed",
  notDetermined: "Not asked yet",
  unsupported: "Not available",
};

/** Index, meaning search, permissions, appearance and about, as ruled rows. */
export function SettingsModal({ initial, onClose, onHelp }: { initial: AppStatus; onClose: () => void; onHelp: () => void }) {
  const [status, setStatus] = useState(initial);
  const [version, setVersion] = useState<string | null>(null);
  const [confirmRebuild, setConfirmRebuild] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const style = useHighlightStyle();

  // Live numbers while the modal is open (meaning search progress, permissions).
  useEffect(() => {
    let live = true;
    const tick = () => api.status().then((s) => live && setStatus(s), () => {});
    tick();
    const t = setInterval(tick, POLL_MS);
    api.appVersion().then((v) => live && setVersion(v), () => {});
    return () => {
      live = false;
      clearInterval(t);
    };
  }, []);

  const s = status.stats;
  const range =
    s.oldestMs && s.newestMs ? `${monthYear.format(s.oldestMs)} to ${monthYear.format(s.newestMs)}` : "None yet";
  // Overall and monotonic: embedded windows over all windows.
  const meaningPct = s.windows > 0 ? Math.min(100, Math.floor((s.embeddedWindows / s.windows) * 100)) : 0;
  const meaningState =
    status.semantic === "ready"
      ? "Ready"
      : status.semantic === "embedding"
        ? `Building, ${meaningPct}%`
        : status.semantic === "downloadingModel"
          ? "Downloading the model"
          : "Not available yet";

  const run = (p: Promise<unknown>) => p.catch((e) => setNote(asCommandError(e).message));

  return (
    <Modal title="Settings" onClose={onClose}>
      <Section title="Index">
        <Row label="Messages">{formatCount(s.messages)}</Row>
        <Row label="Conversations">{formatCount(s.chats)}</Row>
        <Row label="Attachments">{formatCount(s.attachments)}</Row>
        <Row label="Covers">{range}</Row>
        <Row label="Index size">{formatBytes(s.indexBytes)}</Row>
        <div className="modal-actions">
          <button className="btn" onClick={() => void run(api.revealIndex())}>
            Show Index Folder
          </button>
          {!confirmRebuild ? (
            <button className="btn" onClick={() => setConfirmRebuild(true)}>
              Rebuild Index
            </button>
          ) : (
            <span className="confirm">
              <span className="confirm-text">Reread every message? Search keeps working meanwhile.</span>
              <button className="btn" onClick={() => setConfirmRebuild(false)}>
                Cancel
              </button>
              <button
                className="btn primary"
                onClick={() => {
                  setConfirmRebuild(false);
                  void run(api.reindex());
                }}
              >
                Rebuild
              </button>
            </span>
          )}
        </div>
      </Section>

      <Section title="Meaning search">
        <Row label="State">{meaningState}</Row>
        {status.semantic === "embedding" && (
          <div className="meter" aria-label={`Meaning search ${meaningPct}%`}>
            <span style={{ width: `${meaningPct}%` }} />
          </div>
        )}
        <Row label="Model">{status.modelId ?? "None yet"}</Row>
        <p className="modal-note">Meaning search runs entirely on this Mac. Word search works while it builds.</p>
      </Section>

      <Section title="Permissions">
        <Row label="Full Disk Access" detail="Needed to read your Messages history.">
          <span className="perm">
            <span className={`perm-state${status.access === "ok" ? " ok" : ""}`}>{status.access === "ok" ? "Allowed" : "Not allowed"}</span>
            <button className="btn" onClick={() => void run(api.openFullDiskAccessSettings())}>
              Open Settings
            </button>
          </span>
        </Row>
        <Row label="Contacts" detail="Shows names and photos instead of numbers.">
          <span className="perm">
            <span className={`perm-state${status.contacts === "authorized" ? " ok" : ""}`}>{CONTACTS[status.contacts]}</span>
            {status.contacts !== "unsupported" && (
              <button className="btn" onClick={() => void run(api.openContactsSettings())}>
                Open Settings
              </button>
            )}
          </span>
        </Row>
      </Section>

      <Section title="Appearance">
        <Row label="Match highlight" detail={HIGHLIGHT_STYLES.find((h) => h.id === style)?.about}>
          <div className="segments" role="radiogroup" aria-label="Match highlight">
            {HIGHLIGHT_STYLES.map((h) => (
              <button
                key={h.id}
                role="radio"
                aria-checked={style === h.id}
                className={`segment${style === h.id ? " is-active" : ""}`}
                onClick={() => setHighlightStyle(h.id)}
              >
                {h.label}
              </button>
            ))}
          </div>
        </Row>
      </Section>

      <Section title="About">
        <Row label="Messages Search">{version ? `Version ${version}` : ""}</Row>
        <Row label="Made by Justin Gluska" detail="Questions, feedback or support">
          <button className="btn" onClick={() => openLink("https://x.com/gluska")}>
            @gluska on X
          </button>
        </Row>
        <Row label="Source code" detail="Report bugs and request features on GitHub">
          <button className="btn" onClick={() => openLink("https://github.com/justingluska/messages-search")}>
            GitHub
          </button>
        </Row>
        <Row label="License">Open source, GPL-3.0</Row>
        <Row label="Privacy" detail="Reads your history read-only. Nothing leaves this Mac." />
        <Row label="Keyboard shortcuts">
          <button className="btn" onClick={onHelp}>
            Show
          </button>
        </Row>
      </Section>
      {note && <p className="modal-note is-error">{note}</p>}
    </Modal>
  );
}
