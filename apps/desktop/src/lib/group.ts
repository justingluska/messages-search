// Search hits grouped by conversation, Google style: groups keep the order of
// their best (first-ranked) hit, and each group keeps its hits in rank order.

import type { Person, SearchHit } from "./types";

export interface HitGroup {
  key: string;
  chatId: number | null;
  title: string;
  isGroup: boolean;
  people: Person[];
  hits: SearchHit[];
  latestMs: number;
}

/** Snippets shown per conversation before "Show all". */
export const GROUP_PREVIEW = 3;

export function groupHits(hits: SearchHit[]): HitGroup[] {
  const byKey = new Map<string, HitGroup>();
  const out: HitGroup[] = [];
  for (const h of hits) {
    const key = h.chatId == null ? `m${h.messageId}` : `c${h.chatId}`;
    let g = byKey.get(key);
    if (!g) {
      g = { key, chatId: h.chatId, title: h.chatTitle, isGroup: h.isGroup, people: h.people ?? [], hits: [], latestMs: h.dateMs };
      byKey.set(key, g);
      out.push(g);
    }
    g.hits.push(h);
    if (h.dateMs > g.latestMs) g.latestMs = h.dateMs;
  }
  return out;
}

/** The hits visible on screen, in order, for ↑/↓. */
export function visibleHits(groups: HitGroup[], expanded: ReadonlySet<string>): SearchHit[] {
  const out: SearchHit[] = [];
  for (const g of groups) out.push(...(expanded.has(g.key) ? g.hits : g.hits.slice(0, GROUP_PREVIEW)));
  return out;
}
