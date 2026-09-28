import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { api, asCommandError, attachmentSrc } from "../lib/api";
import { formatBytes, fullDate } from "../lib/format";
import { closeLightbox, stepLightbox, useLightbox, type LightboxItem } from "../lib/lightbox";
import { showToast } from "../lib/toast";
import { Icon } from "./Icon";

/**
 * Full-window attachment viewer: large image (contain) or a playing video on
 * a dark scrim; ← / → step through the set, Esc closes. Files that live only
 * in iCloud show a "Not downloaded on this Mac" state instead.
 */
export function LightboxHost() {
  const s = useLightbox();
  if (!s) return null;
  // Portaled to <body> so no scrolled or transformed ancestor can offset it.
  return createPortal(<Lightbox key="lb" items={s.items} index={s.index} />, document.body);
}

function Lightbox({ items, index }: { items: LightboxItem[]; index: number }) {
  const item = items[index];
  const root = useRef<HTMLDivElement>(null);
  const opener = useRef<Element | null>(null);

  useEffect(() => {
    opener.current = document.activeElement;
    root.current?.focus();
    return () => {
      if (opener.current instanceof HTMLElement) opener.current.focus({ preventScroll: true });
    };
  }, []);

  const run = (p: Promise<unknown>, ok?: string) =>
    p.then(
      () => ok && showToast(ok),
      (e) => showToast(asCommandError(e).message, { tone: "error" }),
    );

  const save = () =>
    api.saveAttachment(item.id).then(
      () => showToast("Saved to Downloads", { action: { label: "Show", run: () => void api.revealAttachment(item.id) } }),
      (e) => showToast(asCommandError(e).message, { tone: "error" }),
    );

  const onDisk = !!item.path;
  const who = item.fromMe ? "You" : item.sender ?? "Unknown";

  return (
    <div
      className="lightbox"
      ref={root}
      tabIndex={-1}
      role="dialog"
      aria-modal="true"
      aria-label={item.filename ?? "Attachment"}
      onKeyDown={(e) => {
        if (e.key === "Escape") closeLightbox();
        else if (e.key === "ArrowLeft") stepLightbox(-1);
        else if (e.key === "ArrowRight") stepLightbox(1);
        else return;
        e.preventDefault();
        e.stopPropagation();
      }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) closeLightbox();
      }}
    >
      <header className="lb-bar">
        <div className="lb-meta">
          <div className="lb-name">{item.filename ?? (item.kind === "video" ? "Video" : "Photo")}</div>
          <div className="lb-sub">
            {formatBytes(item.bytes)} · {who} · {fullDate(item.dateMs)}
            {items.length > 1 && ` · ${index + 1} of ${items.length}`}
          </div>
        </div>
        <div className="lb-tools">
          <button className="lb-btn" disabled={!onDisk} onClick={() => void run(api.copyAttachment(item.id), "Copied")}>
            <Icon name="copy" size={16} />
            Copy
          </button>
          <button className="lb-btn" disabled={!onDisk} onClick={() => void save()}>
            <Icon name="download" size={16} />
            Save to Downloads
          </button>
          <button className="lb-btn" disabled={!onDisk} onClick={() => void run(api.openAttachment(item.id))}>
            <Icon name="external" size={16} />
            Open
          </button>
          <button className="lb-btn" disabled={!onDisk} onClick={() => void run(api.revealAttachment(item.id))}>
            <Icon name="folder" size={16} />
            Show in Finder
          </button>
          <button className="lb-btn lb-icon" onClick={closeLightbox} aria-label="Close (Esc)">
            <Icon name="x" size={16} />
          </button>
        </div>
      </header>

      <div className="lb-stage" onMouseDown={(e) => e.target === e.currentTarget && closeLightbox()}>
        {onDisk ? <Media key={item.id} item={item} /> : <NotOnMac />}
      </div>

      {index > 0 && (
        <button className="lb-nav lb-prev" onClick={() => stepLightbox(-1)} aria-label="Previous (←)">
          <Icon name="back" size={20} />
        </button>
      )}
      {index < items.length - 1 && (
        <button className="lb-nav lb-next" onClick={() => stepLightbox(1)} aria-label="Next (→)">
          <Icon name="forward" size={20} />
        </button>
      )}
    </div>
  );
}

function Media({ item }: { item: LightboxItem }) {
  const [failed, setFailed] = useState(false);
  if (failed) {
    return (
      <div className="lb-empty">
        <Icon name="image" size={28} />
        <div className="lb-empty-title">No preview for this file</div>
        <div className="lb-empty-sub">Open it to view it in its app.</div>
        <button className="lb-btn" onClick={() => void api.openAttachment(item.id)}>
          <Icon name="external" size={16} />
          Open
        </button>
      </div>
    );
  }
  const src = attachmentSrc(item.path!);
  if (item.kind === "video") {
    return <video className="lb-media" src={src} controls autoPlay playsInline onError={() => setFailed(true)} />;
  }
  return <img className="lb-media" src={src} alt={item.filename ?? "Photo"} draggable={false} onError={() => setFailed(true)} />;
}

function NotOnMac() {
  return (
    <div className="lb-empty">
      <Icon name="cloud" size={28} />
      <div className="lb-empty-title">Not downloaded on this Mac</div>
      <div className="lb-empty-sub">This file is in iCloud. Open the conversation in Messages to download it.</div>
    </div>
  );
}
