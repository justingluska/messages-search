import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { api } from "../../lib/api";
import { useHighlightStyle } from "../../lib/prefs";
import type { MessageView } from "../../lib/types";
import { MessageRow, buildRows, estimateRow } from "./MessageRow";
import { GalleryContext } from "./gallery";
import { viewable, type LightboxItem } from "../../lib/lightbox";

const AROUND = 40;
const PAGE = 60;
/** Start loading more when this close (in rows) to either end. */
const EDGE = 8;
/** Loaded messages are capped; paging past the cap drops the far end (it reloads if you scroll back). */
const MAX_LOADED = 1000;

interface Loaded {
  chatId: number;
  msgs: MessageView[];
  /** Nothing older / newer left to load. */
  start: boolean;
  end: boolean;
}

const EMPTY: Loaded = { chatId: -1, msgs: [], start: true, end: true };

/**
 * Messages-style transcript of one chat around a search hit. Virtualized with
 * measured, variable row heights; loads older/newer pages at the edges and
 * keeps the visible message still when older ones are prepended.
 */
export function Transcript({
  chatId,
  hitId,
  isGroup,
  highlight,
  version,
  scrollerRef,
  onEscape,
}: {
  chatId: number;
  hitId: number;
  isGroup: boolean;
  highlight: RegExp | null;
  /** Bumps on `index-changed`. */
  version: number;
  scrollerRef: React.RefObject<HTMLDivElement | null>;
  onEscape: () => void;
}) {
  const [data, setData] = useState<Loaded>(EMPTY);
  const [center, setCenter] = useState<{ id: number; n: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const style = useHighlightStyle();
  /** When the current hit was located (drives the Glow style's one-shot fade). */
  const [hitAt, setHitAt] = useState(0);
  /** Focus style: the rest of the chat is dimmed until the user scrolls, clicks or types. */
  const [dimmed, setDimmed] = useState(false);
  const dataRef = useRef(data);
  dataRef.current = data;
  /** Bumps whenever `data` is replaced wholesale; page responses from an older generation are dropped. */
  const gen = useRef(0);
  const busy = useRef({ older: false, newer: false });
  const anchor = useRef<{ key: number; offset: number } | null>(null);
  const settled = useRef(false);
  /** False from when a page lands until the next user scroll. */
  const edgeReady = useRef(true);

  const rows = useMemo(() => buildRows(data.msgs, isGroup), [data.msgs, isGroup]);

  // The lightbox steps through every photo and video loaded in this chat.
  const gallery = useCallback((): LightboxItem[] => {
    const out: LightboxItem[] = [];
    for (const m of dataRef.current.msgs)
      for (const a of m.attachments)
        if (viewable(a.kind)) out.push({ ...a, sender: m.sender, fromMe: m.fromMe, dateMs: m.dateMs });
    return out;
  }, []);

  const v = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollerRef.current,
    estimateSize: (i) => estimateRow(rows[i]),
    getItemKey: (i) => rows[i].m.id,
    overscan: 10,
    paddingStart: 14,
    paddingEnd: 22,
  });
  // A row entirely above the viewport changing size (an image finishing
  // loading, a first measurement) must never move what's on screen, in either
  // scroll direction. The library's default skips re-measurements while
  // scrolling up, which makes the transcript jump as photos load.
  v.shouldAdjustScrollPositionOnItemSizeChange = (item, _delta, inst) => item.end <= (inst.scrollOffset ?? 0);

  // Load (or reuse) the messages around the hit.
  useEffect(() => {
    settled.current = false;
    const cur = dataRef.current;
    if (cur.chatId === chatId && cur.msgs.some((m) => m.id === hitId)) {
      setCenter((c) => ({ id: hitId, n: (c?.n ?? 0) + 1 }));
      return;
    }
    const my = ++gen.current;
    busy.current = { older: false, newer: false };
    api.messagesAround(hitId, AROUND, AROUND).then(
      (msgs) => {
        if (my !== gen.current) return;
        const i = msgs.findIndex((m) => m.id === hitId);
        setError(null);
        setData({ chatId, msgs, start: i < AROUND, end: msgs.length - 1 - i < AROUND });
        setCenter((c) => ({ id: hitId, n: (c?.n ?? 0) + 1 }));
      },
      (e) => {
        if (my === gen.current) setError(e?.message ?? String(e));
      },
    );
  }, [chatId, hitId]);

  // Center the hit once its rows exist.
  useLayoutEffect(() => {
    if (!center) return;
    const idx = rows.findIndex((r) => r.m.id === center.id);
    if (idx < 0) return;
    v.scrollToIndex(idx, { align: "center" });
    setHitAt(performance.now());
    setDimmed(true);
    // Edge loading waits until the initial positioning has settled.
    const t = setTimeout(() => (settled.current = true), 250);
    return () => clearTimeout(t);
  }, [center]);

  // Keep the anchor message still after a prepend.
  useLayoutEffect(() => {
    const a = anchor.current;
    const el = scrollerRef.current;
    if (!a || !el) return;
    anchor.current = null;
    const idx = rows.findIndex((r) => r.m.id === a.key);
    if (idx < 0) return;
    // getOffsetForIndex recomputes measurements first, so rows measured during
    // this commit (the prepended ones) are already accounted for.
    const at = v.getOffsetForIndex(idx, "start");
    if (at) el.scrollTop = at[0] - a.offset;
  }, [rows, v, scrollerRef]);

  const loadOlder = useCallback(() => {
    const d = dataRef.current;
    if (busy.current.older || d.start || !d.msgs.length) return;
    busy.current.older = true;
    const my = gen.current;
    const first = d.msgs[0];
    api.messagesPage(d.chatId, first.dateMs, first.id, false, PAGE).then(
      (older) => {
        if (my !== gen.current) return;
        busy.current.older = false;
        const el = scrollerRef.current;
        if (el && older.length) {
          const top = el.scrollTop;
          // Not row 0: its timestamp/sender header can change once older rows exist above it.
          const vis = v.getVirtualItems().find((it) => it.index > 0 && it.end > top);
          if (vis) anchor.current = { key: vis.key as number, offset: vis.start - top };
        }
        edgeReady.current = false;
        setData((p) => {
          if (!older.length) return { ...p, start: true };
          let msgs = [...older, ...p.msgs];
          let end = p.end;
          // Over the cap: drop the newest end (below the viewport, so nothing moves).
          if (msgs.length > MAX_LOADED) {
            msgs = msgs.slice(0, MAX_LOADED);
            end = false;
          }
          return { ...p, msgs, start: older.length < PAGE, end };
        });
      },
      () => (busy.current.older = false),
    );
  }, [v, scrollerRef]);

  const loadNewer = useCallback(() => {
    const d = dataRef.current;
    if (busy.current.newer || !d.msgs.length) return;
    busy.current.newer = true;
    const my = gen.current;
    const last = d.msgs[d.msgs.length - 1];
    api.messagesPage(d.chatId, last.dateMs, last.id, true, PAGE).then(
      (newer) => {
        if (my !== gen.current) return;
        busy.current.newer = false;
        const cur = dataRef.current;
        // Over the cap, the oldest end is dropped: that removes rows above the
        // viewport, so anchor the visible message exactly as for a prepend.
        if (newer.length && cur.msgs.length + newer.length > MAX_LOADED) {
          const el = scrollerRef.current;
          if (el) {
            const top = el.scrollTop;
            const vis = v.getVirtualItems().find((it) => it.index > 0 && it.end > top);
            if (vis) anchor.current = { key: vis.key as number, offset: vis.start - top };
          }
        }
        edgeReady.current = false;
        setData((p) => {
          if (!newer.length) return { ...p, end: true };
          let msgs = [...p.msgs, ...newer];
          let start = p.start;
          if (msgs.length > MAX_LOADED) {
            msgs = msgs.slice(msgs.length - MAX_LOADED);
            start = false;
          }
          return { ...p, msgs, end: newer.length < PAGE, start };
        });
      },
      () => (busy.current.newer = false),
    );
  }, [v, scrollerRef]);

  const items = v.getVirtualItems();
  const firstIdx = items[0]?.index ?? 0;
  const lastIdx = items[items.length - 1]?.index ?? 0;

  useEffect(() => {
    if (!settled.current || rows.length === 0) return;
    // After a page lands, the range above was computed before the anchor moved
    // the scroll position; wait for a real scroll before loading again, or
    // stale indices cascade into back-to-back loads (and, with the cap, into
    // loads that undo each other).
    if (!edgeReady.current) return;
    if (firstIdx < EDGE && !data.start) loadOlder();
    else if (lastIdx > rows.length - 1 - EDGE && !data.end) loadNewer();
  }, [firstIdx, lastIdx, rows.length, data.start, data.end, loadOlder, loadNewer]);

  // New messages arrived: if the newest end is loaded, fetch past it.
  useEffect(() => {
    if (version > 0 && dataRef.current.end) loadNewer();
  }, [version, loadNewer]);

  return (
    <GalleryContext.Provider value={gallery}>
      <div
        className={`transcript v-scroll hl-${style}${dimmed && style === "focus" ? " is-dimmed" : ""}`}
        ref={scrollerRef}
        tabIndex={0}
        aria-label="Conversation"
        onScroll={() => (edgeReady.current = true)}
      onWheel={dimmed ? () => setDimmed(false) : undefined}
        onMouseDown={dimmed ? () => setDimmed(false) : undefined}
        onKeyDown={(e) => {
          if (dimmed) setDimmed(false);
          if (e.key === "Escape" || e.key === "ArrowLeft") {
            e.preventDefault();
            e.stopPropagation();
            onEscape();
          }
        }}
      >
        {error && <div className="transcript-error">{error}</div>}
        <div style={{ height: v.getTotalSize(), position: "relative" }}>
          {items.map((it) => {
            const r = rows[it.index];
            return (
              <div key={it.key} data-index={it.index} data-id={it.key as number} ref={v.measureElement} className="msg-slot" style={{ transform: `translateY(${it.start}px)` }}>
                <MessageRow key={r.m.id === hitId ? `hit-${hitAt}` : "row"} row={r} isHit={r.m.id === hitId} hitAt={hitAt} highlight={highlight} />
              </div>
            );
          })}
        </div>
      </div>
    </GalleryContext.Provider>
  );
}
