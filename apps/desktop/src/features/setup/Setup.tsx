import { useEffect, useState } from "react";
import appIcon from "../../assets/app-icon.png";
import { Icon } from "../../components/Icon";
import { api } from "../../lib/api";
import type { AppStatus } from "../../lib/types";

const POLL_MS = 2000;

/**
 * Shown while the app can't read the Messages database. Polls `status` every
 * 2 s while visible, so granting Full Disk Access moves on by itself.
 */
export function Setup({ status, onStatus }: { status: AppStatus; onStatus: (s: AppStatus) => void }) {
  const [checking, setChecking] = useState(false);
  const [asked, setAsked] = useState(false);

  useEffect(() => {
    let live = true;
    const t = setInterval(() => {
      if (document.visibilityState !== "visible") return;
      api.status().then((s) => live && s.access !== status.access && onStatus(s));
    }, POLL_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [status.access, onStatus]);

  const recheck = () => {
    setChecking(true);
    api
      .status()
      .then(onStatus)
      .finally(() => setTimeout(() => setChecking(false), 400));
  };

  if (status.access === "noMessagesDb") {
    return (
      <div className="setup" data-tauri-drag-region>
        <div className="setup-card">
          <div className="empty-art"><img className="setup-icon" src={appIcon} alt="" width={72} height={72} /></div>
          <h1>No messages found</h1>
          <p className="setup-lead">
            There is no Messages history on this Mac yet. Open Messages, sign in with your Apple Account, and turn on Messages in
            iCloud to bring your history here.
          </p>
          <div className="setup-actions">
            <button className="btn primary" onClick={recheck} disabled={checking}>
              {checking ? "Checking" : "Check Again"}
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="setup" data-tauri-drag-region>
      <div className="setup-card">
        <div className="empty-art"><img className="setup-icon" src={appIcon} alt="" width={72} height={72} /></div>
        <h1>Allow access to your messages</h1>
        <p className="setup-lead">
          Messages Search builds a private search index from the Messages history on this Mac. macOS keeps that history behind
          Full Disk Access, so it needs your permission first.
        </p>
        <ul className="setup-points">
          <li>
            <Icon name="lock" size={16} />
            <span><strong>Private.</strong> Everything stays on this Mac. Nothing is uploaded.</span>
          </li>
          <li>
            <Icon name="check" size={16} />
            <span><strong>Read-only.</strong> It never changes or sends messages.</span>
          </li>
          <li>
            <Icon name="doc" size={16} />
            <span><strong>Open source.</strong> Anyone can check what it does.</span>
          </li>
        </ul>
        <ol className="setup-steps">
          <li>In System Settings, open Privacy &amp; Security › Full Disk Access.</li>
          <li>Turn on Messages Search, then choose Quit &amp; Reopen if asked.</li>
        </ol>
        <div className="setup-actions">
          <button
            className="btn primary"
            onClick={() => {
              setAsked(true);
              void api.openFullDiskAccessSettings();
            }}
          >
            Open System Settings
          </button>
          <button className="btn" onClick={recheck} disabled={checking}>
            {checking ? "Checking" : "I've Granted Access"}
          </button>
        </div>
        {asked && <div className="setup-wait">This window continues on its own once access is on.</div>}
      </div>
    </div>
  );
}
