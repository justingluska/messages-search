import { memo, useContext, useState } from "react";
import { Avatar } from "../../components/Avatar";
import { Icon } from "../../components/Icon";
import { api, attachmentSrc, openLink } from "../../lib/api";
import { formatBytes, shortName, transcriptDate } from "../../lib/format";
import type { AttachmentView, MessageView, ReactionView } from "../../lib/types";
import { openLightbox, viewable } from "../../lib/lightbox";
import { GalleryContext } from "./gallery";

export interface Row {
  m: MessageView;
  /** Centered timestamp above (first row, or more than an hour since the previous message). */
  showTime: boolean;
  /** Sender name above the bubble (group chats, first of a run). */
  showName: boolean;
  /** Last bubble of a run by the same sender: draw the tail (and the avatar in groups). */
  tail: boolean;
  /** Received message in a group: reserve the avatar column. */
  avatarSlot: boolean;
  /** The replied-to message when loaded; null when not loaded; undefined when not a reply. */
  replyTo: MessageView | null | undefined;
  /** Space above, px (tight within a run, looser between senders). */
  gap: number;
}

const RUN_GAP_MS = 15 * 60_000;
const TIME_GAP_MS = 60 * 60_000;

const isBubble = (m: MessageView) => m.kind !== "system" && !m.unsent;
const sameRun = (a: MessageView | undefined, b: MessageView) =>
  !!a && isBubble(a) && isBubble(b) && a.fromMe === b.fromMe && a.senderHandleId === b.senderHandleId && b.dateMs - a.dateMs < RUN_GAP_MS;

export function buildRows(msgs: MessageView[], isGroup: boolean): Row[] {
  const byGuid = new Map(msgs.map((m) => [m.guid, m]));
  const rows: Row[] = new Array(msgs.length);
  for (let i = 0; i < msgs.length; i++) {
    const m = msgs[i];
    const prev = msgs[i - 1];
    const next = msgs[i + 1];
    const showTime = !prev || m.dateMs - prev.dateMs > TIME_GAP_MS;
    const withPrev = !showTime && sameRun(prev, m);
    const withNext = !!next && next.dateMs - m.dateMs <= TIME_GAP_MS && sameRun(m, next);
    const received = isGroup && !m.fromMe && isBubble(m);
    rows[i] = {
      m,
      showTime,
      showName: received && !withPrev,
      tail: !withNext,
      avatarSlot: received,
      replyTo: m.replyToGuid ? byGuid.get(m.replyToGuid) ?? null : undefined,
      gap: showTime ? 4 : withPrev ? 2 : 10,
    };
  }
  return rows;
}

/** Rough height before measuring, so the scrollbar is roughly right. */
export function estimateRow(r: Row): number {
  let h = r.gap + (r.showTime ? 30 : 0) + (r.showName ? 16 : 0) + (r.m.reactions.length ? 12 : 0) + (r.replyTo !== undefined ? 30 : 0);
  if (!isBubble(r.m)) return h + 20;
  for (const a of r.m.attachments) h += a.kind === "image" ? 240 : a.kind === "sticker" ? 120 : 56;
  if (r.m.text) h += 18 + Math.ceil(r.m.text.length / 48) * 18;
  if (r.m.edited) h += 16;
  return h;
}

const URL_RE = /((?:https?:\/\/|www\.)[^\s<]+[^\s<.,:;"')\]!?])/gi;
const LINK_ONLY = /^(?:https?:\/\/|www\.)\S+$/i;

export const MessageRow = memo(function MessageRow({
  row,
  isHit,
  hitAt,
  highlight,
}: {
  row: Row;
  isHit: boolean;
  /** performance.now() when this hit was located; the glow runs from then, so a row remounted by scrolling doesn't replay it. */
  hitAt: number;
  highlight: RegExp | null;
}) {
  const { m } = row;
  // Captured once per mount (the parent remounts the row when it becomes the hit).
  const [glowDelay] = useState(() => (isHit ? -Math.max(0, performance.now() - hitAt) : 0));
  const side = m.fromMe ? "out" : "in";
  const name = m.sender ?? "Unknown";

  let body: React.ReactNode;
  if (m.unsent) {
    body = <div className="msg-system">{m.fromMe ? "You" : shortName(name)} unsent a message</div>;
  } else if (m.kind === "system") {
    body = <div className="msg-system">{m.text}</div>;
  } else {
    const parts: React.ReactNode[] = [];
    for (const a of m.attachments) parts.push(<Attachment key={`a${a.id}`} a={a} side={side} />);
    const text = m.text?.trim();
    if (text && LINK_ONLY.test(text)) parts.push(<LinkCard key="link" url={text} />);
    else if (text) {
      const sms = m.fromMe && m.service === "SMS";
      parts.push(
        <div key="text" className={`bubble ${side}${sms ? " sms" : ""}${row.tail ? " tail" : ""}`}>
          <RichText text={m.text!} highlight={highlight} />
        </div>,
      );
    }
    if (m.kind === "app" && parts.length === 0) parts.push(<div key="app" className={`bubble app ${side}`}>App message</div>);

    body = (
      <div className={`msg-line ${side}`}>
        {row.avatarSlot && <div className="msg-avatar">{row.tail && <Avatar name={name} src={m.senderAvatar} size={28} />}</div>}
        <div className="msg-col">
          {row.showName && <div className="msg-name">{shortName(name)}</div>}
          {row.replyTo !== undefined && <ReplyQuote target={row.replyTo} side={side} />}
          {parts.map((p, i) => (
            <div key={i} className={`msg-part${isHit ? " is-hit" : ""}`} style={isHit ? { animationDelay: `${glowDelay}ms` } : undefined}>
              {p}
              {i === 0 && m.reactions.length > 0 && <Reactions reactions={m.reactions} side={side} />}
            </div>
          ))}
          {m.edited && <div className="msg-meta">Edited</div>}
          {isHit && <div className="msg-meta msg-match">Match</div>}
        </div>
      </div>
    );
  }

  return (
    <div className={`msg-row${m.reactions.length ? " has-reactions" : ""}`} style={{ paddingTop: row.gap }}>
      {row.showTime && <div className="msg-time">{transcriptDate(m.dateMs)}</div>}
      <div className={`msg-body${isHit ? " is-hit-row" : ""}`}>{body}</div>
    </div>
  );
});

/** http(s) URL for a linkified piece ("example.com/x" → https://…). */
function toHref(p: string): string {
  return /^https?:\/\//i.test(p) ? p : `https://${p}`;
}

/**
 * Link handlers: every click (including middle and ⌘-click, which would
 * otherwise navigate the app's own webview) goes to the default browser.
 */
function linkProps(href: string) {
  const go = (e: React.MouseEvent) => {
    e.preventDefault();
    openLink(href);
  };
  return { href, onClick: go, onAuxClick: go };
}

function RichText({ text, highlight }: { text: string; highlight: RegExp | null }) {
  const pieces = text.split(URL_RE);
  return (
    <>
      {pieces.map((p, i) =>
        i % 2 === 1 ? (
          <a key={i} {...linkProps(toHref(p))}>
            <Highlighted text={p} re={highlight} />
          </a>
        ) : (
          <Highlighted key={i} text={p} re={highlight} />
        ),
      )}
    </>
  );
}

function Highlighted({ text, re }: { text: string; re: RegExp | null }) {
  if (!re || !text) return <>{text}</>;
  const parts = text.split(re);
  if (parts.length === 1) return <>{text}</>;
  return <>{parts.map((p, i) => (i % 2 === 1 ? <mark key={i}>{p}</mark> : p))}</>;
}

function LinkCard({ url }: { url: string }) {
  const href = toHref(url);
  let host = url;
  let rest = "";
  try {
    const u = new URL(href);
    host = u.hostname.replace(/^www\./, "");
    rest = (u.pathname + u.search).replace(/^\/$/, "");
  } catch {
    // Not a parseable URL: show it as typed.
  }
  return (
    <a className="link-card" {...linkProps(href)}>
      <span className="link-host">{host}</span>
      {rest && <span className="link-path">{rest}</span>}
    </a>
  );
}

function Attachment({ a, side }: { a: AttachmentView; side: "in" | "out" }) {
  const [failed, setFailed] = useState(false);
  const gallery = useContext(GalleryContext);
  const view = () => openLightbox(gallery(), a.id);
  const visual = (a.kind === "image" || a.kind === "sticker") && a.path && !failed;
  if (visual) {
    return (
      <img
        className={a.kind === "sticker" ? "msg-sticker" : "msg-image"}
        src={attachmentSrc(a.path!)}
        alt={a.filename ?? "Photo"}
        loading="lazy"
        decoding="async"
        draggable={false}
        onError={() => setFailed(true)}
        onClick={view}
      />
    );
  }
  const label =
    a.kind === "audio" ? "Audio Message" : a.filename ?? (a.kind === "video" ? "Video" : a.kind === "image" ? "Photo" : "Attachment");
  const icon = a.kind === "audio" ? "wave" : a.kind === "video" ? "play" : "doc";
  return (
    <button
      className={`msg-file ${side}`}
      onClick={viewable(a.kind) ? view : () => void api.revealAttachment(a.id)}
      title={viewable(a.kind) ? "View" : "Show in Finder"}
    >
      <span className="msg-file-icon">
        <Icon name={icon} size={16} />
      </span>
      <span className="msg-file-text">
        <span className="msg-file-name">{label}</span>
        <span className="msg-file-size">
          {a.kind === "audio" && a.filename ? `${a.filename} · ` : ""}
          {formatBytes(a.bytes)}
        </span>
      </span>
    </button>
  );
}

function Reactions({ reactions, side }: { reactions: ReactionView[]; side: "in" | "out" }) {
  const shown = reactions.slice(0, 3);
  const who = reactions.map((r) => `${r.fromMe ? "You" : r.sender ?? "Someone"} ${r.emoji}`).join(", ");
  return (
    <div className={`reactions ${side}`} title={who}>
      {shown.map((r, i) => (
        <span key={i} className={`reaction${r.fromMe ? " mine" : ""}`}>
          {r.emoji}
        </span>
      ))}
    </div>
  );
}

function ReplyQuote({ target, side }: { target: MessageView | null; side: "in" | "out" }) {
  const text = target
    ? target.text ?? (target.attachments.length ? (target.attachments[0].kind === "image" ? "Photo" : "Attachment") : "Message")
    : "Earlier message";
  const who = target ? (target.fromMe ? "You" : shortName(target.sender ?? "")) : null;
  return (
    <div className={`reply-quote ${side}`}>
      <Icon name="reply" size={11} />
      <span className="reply-text">
        {who && <span className="reply-who">{who}: </span>}
        {text}
      </span>
    </div>
  );
}
