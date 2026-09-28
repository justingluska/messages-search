import { memo, useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { PeopleAvatar } from "../../components/Avatar";
import { Snippet } from "../../components/Snippet";
import { listDate, shortName } from "../../lib/format";
import { GROUP_PREVIEW, type HitGroup } from "../../lib/group";
import type { SearchHit } from "../../lib/types";

/**
 * Results grouped by conversation in one centered column. Virtualized by
 * group with measured heights, so thousands of hits stay cheap.
 */
export function Results({
  groups,
  expanded,
  onToggle,
  selectedId,
  openId,
  onSelect,
  onOpen,
  scrollRef,
  header,
  markSimilar,
}: {
  groups: HitGroup[];
  expanded: ReadonlySet<string>;
  onToggle: (key: string) => void;
  selectedId: number | null;
  /** The hit shown in the conversation panel, if any. */
  openId: number | null;
  onSelect: (h: SearchHit) => void;
  onOpen: (h: SearchHit) => void;
  scrollRef: React.RefObject<HTMLDivElement | null>;
  header: React.ReactNode;
  /** Label meaning matches; off when every result is one (the header already says so). */
  markSimilar: boolean;
}) {
  const v = useVirtualizer({
    count: groups.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => 70 + Math.min(groups[i].hits.length, GROUP_PREVIEW) * 38 + (groups[i].hits.length > GROUP_PREVIEW ? 32 : 0),
    getItemKey: (i) => groups[i].key,
    overscan: 4,
  });

  // Keep the selected snippet on screen.
  const lastSel = useRef<number | null>(null);
  useEffect(() => {
    if (selectedId == null || selectedId === lastSel.current) return;
    lastSel.current = selectedId;
    const el = scrollRef.current?.querySelector<HTMLElement>(`[data-hit="${selectedId}"]`);
    if (el) {
      el.scrollIntoView({ block: "nearest" });
      return;
    }
    const gi = groups.findIndex((g) => g.hits.some((h) => h.messageId === selectedId));
    if (gi >= 0) v.scrollToIndex(gi, { align: "auto" });
  }, [selectedId, groups, v, scrollRef]);

  return (
    <div className="results-scroll v-scroll" ref={scrollRef}>
      <div className="results-col">
        {header}
        <div style={{ height: v.getTotalSize(), position: "relative" }} role="listbox" aria-label="Results">
          {v.getVirtualItems().map((it) => {
            const g = groups[it.index];
            return (
              <div key={it.key} data-index={it.index} ref={v.measureElement} className="group-slot" style={{ transform: `translateY(${it.start}px)` }}>
                <Group
                  g={g}
                  open={expanded.has(g.key)}
                  onToggle={onToggle}
                  selectedId={selectedId}
                  openId={openId}
                  onSelect={onSelect}
                  onOpen={onOpen}
                  markSimilar={markSimilar}
                />
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

const Group = memo(function Group({
  g,
  open,
  onToggle,
  selectedId,
  openId,
  onSelect,
  onOpen,
  markSimilar,
}: {
  g: HitGroup;
  open: boolean;
  onToggle: (key: string) => void;
  selectedId: number | null;
  openId: number | null;
  onSelect: (h: SearchHit) => void;
  onOpen: (h: SearchHit) => void;
  markSimilar: boolean;
}) {
  const shown = open ? g.hits : g.hits.slice(0, GROUP_PREVIEW);
  const more = g.hits.length - GROUP_PREVIEW;
  return (
    <section className="group">
      <header className="group-head" onClick={() => onOpen(shown[0])}>
        <PeopleAvatar people={g.people} fallback={g.title} size={36} />
        <div className="group-title-wrap">
          <div className="group-title">{g.title}</div>
          <div className="group-meta">
            {g.hits.length} {g.hits.length === 1 ? "match" : "matches"} · {listDate(g.latestMs)}
          </div>
        </div>
      </header>
      <div className="group-hits">
        {shown.map((h) => (
          <HitRow key={h.messageId} h={h} isGroup={g.isGroup} selected={h.messageId === selectedId} isOpen={h.messageId === openId} onSelect={onSelect} onOpen={onOpen} markSimilar={markSimilar} />
        ))}
      </div>
      {more > 0 && (
        <button className="group-more" onClick={() => onToggle(g.key)}>
          {open ? "Show fewer" : `Show all ${g.hits.length}`}
        </button>
      )}
    </section>
  );
});

const HitRow = memo(function HitRow({
  h,
  isGroup,
  selected,
  isOpen,
  onSelect,
  onOpen,
  markSimilar,
}: {
  h: SearchHit;
  isGroup: boolean;
  selected: boolean;
  isOpen: boolean;
  onSelect: (h: SearchHit) => void;
  onOpen: (h: SearchHit) => void;
  markSimilar: boolean;
}) {
  const who = h.fromMe ? "You" : isGroup && h.sender ? shortName(h.sender) : h.sender ? shortName(h.sender) : null;
  return (
    <div
      className={`hit${selected ? " is-selected" : ""}${isOpen ? " is-open" : ""}`}
      data-hit={h.messageId}
      role="option"
      aria-selected={selected}
      onMouseDown={() => onSelect(h)}
      onClick={() => onOpen(h)}
    >
      <div className="hit-snippet">
        {who && <span className="hit-who">{who}: </span>}
        <Snippet text={h.snippet} />
      </div>
      <span className="hit-date">
        {markSimilar && h.matchedBy === "meaning" && (
          <span className="hit-similar" title="Found by similar meaning" aria-label="Similar meaning">
            ≈
          </span>
        )}
        {listDate(h.dateMs)}
      </span>
    </div>
  );
});
