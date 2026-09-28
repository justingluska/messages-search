// Mock search: honors the same operators as ms-core (from:, in:/with:, has:,
// before/after/during, "phrases", -words), marks keyword matches with
// U+0002…U+0003, and adds fake "meaning" hits from a small concept table so
// the UI shows both kinds of result.

import { parseQuery } from "../query";
import type { MatchedBy, MessageView, ParsedQuery, SearchHit, SearchResults } from "../types";
import { CHAT_BY_ID, MESSAGES } from "./data";

const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
const tokens = (s: string) => fold(s).split(/[^\p{L}\p{N}]+/u).filter(Boolean);
const URL_RE = /\bhttps?:\/\/|\bwww\./i;

// Fake embeddings: a query touching a concept pulls in messages that talk
// about it with different words.
const CONCEPTS: { triggers: string[]; related: string[] }[] = [
  { triggers: ["eat", "food", "dinner", "lunch", "restaurant", "hungry", "brunch", "meal", "taco"], related: ["tacos", "brisket", "reservation", "menu", "queso", "ramen", "bbq", "lasagna", "brunch", "dinner", "meal", "food trucks"] },
  { triggers: ["austin", "texas", "atx"], related: ["south congress", "east 6th", "aus", "casa brava", "festival"] },
  { triggers: ["code", "password", "gate", "wifi", "lockbox", "key", "entry"], related: ["gate code", "4471", "lockbox", "wifi", "bluegill"] },
  { triggers: ["flight", "fly", "flying", "land", "landed", "airport", "plane"], related: ["lands at", "landed", "arrivals", "flights", "ua 1432"] },
  { triggers: ["dog", "puppy", "pet"], related: ["biscuit", "vet"] },
  { triggers: ["money", "pay", "owe", "venmo", "split", "cost"], related: ["venmo", "$212", "splitting", "bill", "per person"] },
  { triggers: ["run", "running", "marathon", "race", "miles"], related: ["long run", "pace", "half", "tempo", "track workout", "pr!!!"] },
  { triggers: ["birthday", "bday", "party", "surprise"], related: ["birthday", "cake", "surprise dinner", "gift"] },
  { triggers: ["photo", "picture", "pictures", "shoot", "camera"], related: ["film roll", "golden hour", "sunrise shoot", "print", "photos"] },
  { triggers: ["work", "meeting", "slides", "deck", "plan"], related: ["standup", "offsite", "q3 plan", "launch review", "budget"] },
];

function matchesFilters(m: MessageView, q: ParsedQuery): boolean {
  if (m.kind !== "text" || m.unsent) return false;
  if (q.afterMs !== null && m.dateMs < q.afterMs) return false;
  if (q.beforeMs !== null && m.dateMs >= q.beforeMs) return false;
  for (const f of q.from) {
    const v = fold(f);
    if (v === "me") {
      if (!m.fromMe) return false;
    } else if (m.fromMe || !fold(m.sender ?? "").includes(v)) return false;
  }
  if (q.chat.length) {
    const chat = CHAT_BY_ID.get(m.chatId!)!;
    for (const c of q.chat) {
      const v = fold(c);
      const hay = [chat.title, ...chat.participants.flatMap((p) => [p.name ?? "", p.address])].map(fold);
      if (!hay.some((h) => h.includes(v))) return false;
    }
  }
  for (const h of q.has) {
    const kinds = m.attachments.map((a) => a.kind);
    const ok =
      h === "link" ? URL_RE.test(m.text ?? "") :
      h === "photo" ? kinds.includes("image") :
      h === "attachment" ? kinds.length > 0 :
      kinds.includes(h);
    if (!ok) return false;
  }
  if (q.excluded.length) {
    const toks = tokens(m.text ?? "");
    if (q.excluded.some((x) => tokens(x).some((t) => toks.includes(t)))) return false;
  }
  return true;
}

/** Keyword score (0 = no match): every word must prefix-match a token, every phrase must appear. */
function keywordScore(m: MessageView, words: string[], phrases: string[]): number {
  const text = m.text;
  if (!text) return 0;
  const folded = fold(text);
  const toks = tokens(text);
  let score = 0;
  for (const w of words) {
    const hits = toks.filter((t) => t.startsWith(w)).length;
    if (!hits) return 0;
    score += hits + (toks.includes(w) ? 1 : 0);
  }
  for (const p of phrases) {
    const pt = tokens(p).join(" ");
    if (!pt || !toks.join(" ").includes(pt)) return 0;
    score += 3;
  }
  return score / Math.sqrt(folded.length / 40 + 1);
}

function snippet(text: string, words: string[], phrases: string[]): string {
  // Mark every prefix match of a word and every phrase occurrence.
  const ranges: [number, number][] = [];
  const folded = fold(text);
  // fold() may change length for some characters; the mock data is plain enough that indexes line up.
  const same = folded.length === text.length;
  const src = same ? folded : text.toLowerCase();
  for (const p of phrases) {
    const needle = tokens(p).join(" ");
    let i = src.indexOf(needle);
    while (needle && i >= 0) {
      ranges.push([i, i + needle.length]);
      i = src.indexOf(needle, i + needle.length);
    }
  }
  const re = /[\p{L}\p{N}]+/gu;
  for (let m = re.exec(src); m; m = re.exec(src)) {
    for (const w of words) {
      if (m[0].startsWith(w)) {
        ranges.push([m.index, m.index + m[0].length]);
        break;
      }
    }
  }
  ranges.sort((a, b) => a[0] - b[0]);
  // Window of ~140 chars around the first match.
  const first = ranges[0]?.[0] ?? 0;
  const from = Math.max(0, first - 50);
  const to = Math.min(text.length, from + 160);
  let out = from > 0 ? "…" : "";
  let pos = from;
  for (const [s, e] of ranges) {
    if (s < pos || e > to) continue;
    out += text.slice(pos, s) + "\u0002" + text.slice(s, e) + "\u0003";
    pos = e;
  }
  out += text.slice(pos, to) + (to < text.length ? "…" : "");
  return out;
}

function plainSnippet(m: MessageView): string {
  if (m.text) return m.text.length > 160 ? m.text.slice(0, 160) + "…" : m.text;
  const a = m.attachments[0];
  if (!a) return "";
  return a.kind === "image" ? "Photo" : a.kind === "video" ? "Video" : a.kind === "audio" ? "Audio Message" : a.filename ?? "Attachment";
}

function hit(m: MessageView, snippetText: string, matchedBy: MatchedBy, score: number): SearchHit {
  const chat = CHAT_BY_ID.get(m.chatId!)!;
  return {
    messageId: m.id,
    chatId: m.chatId,
    chatTitle: chat.title,
    isGroup: chat.isGroup,
    fromMe: m.fromMe,
    sender: m.sender,
    dateMs: m.dateMs,
    snippet: snippetText,
    matchedBy,
    attachmentCount: m.attachments.length,
    score,
    people: chat.participants.slice(0, 4),
  };
}

export function mockSearch(query: string, limit: number, semanticReady: boolean): SearchResults {
  const t0 = performance.now();
  const q = parseQuery(query);
  const words = q.words.flatMap(tokens);
  const phrases = q.phrases;
  const hasText = words.length > 0 || phrases.some((p) => tokens(p).length);
  const candidates = MESSAGES.filter((m) => matchesFilters(m, q));

  if (!hasText) {
    const hits = (q.from.length || q.chat.length || q.has.length || q.afterMs !== null || q.beforeMs !== null ? candidates : [])
      .slice(-limit)
      .reverse()
      .map((m) => hit(m, plainSnippet(m), "filter", 0));
    return { hits, query: q, semantic: semanticReady, tookMs: performance.now() - t0 };
  }

  const keyword = candidates
    .map((m) => ({ m, s: keywordScore(m, words, phrases) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s || b.m.dateMs - a.m.dateMs);

  const fused = new Map<number, { m: MessageView; rrf: number; kw: boolean; mean: boolean }>();
  keyword.forEach(({ m }, rank) => fused.set(m.id, { m, rrf: 1 / (60 + rank), kw: true, mean: false }));

  if (semanticReady) {
    const related = new Set<string>();
    const allWords = [...words, ...phrases.flatMap(tokens)];
    for (const c of CONCEPTS) {
      if (c.triggers.some((t) => allWords.some((w) => t.startsWith(w) && w.length >= 3))) c.related.forEach((r) => related.add(r));
    }
    if (related.size) {
      const scored = candidates
        .map((m) => {
          const f = fold(m.text ?? "");
          let s = 0;
          for (const r of related) if (f.includes(r)) s += r.length > 6 ? 2 : 1;
          return { m, s };
        })
        .filter((x) => x.s > 0)
        .sort((a, b) => b.s - a.s || b.m.dateMs - a.m.dateMs);
      // Windows, not rows, are embedded: one hit per distinct text is closer to the real thing.
      const seen = new Set<string>();
      const meaning = scored.filter(({ m }) => {
        const k = fold(m.text ?? String(m.id));
        if (seen.has(k)) return false;
        seen.add(k);
        return true;
      }).slice(0, 20);
      meaning.forEach(({ m }, rank) => {
        const e = fused.get(m.id);
        if (e) {
          e.rrf += 1 / (60 + rank);
          e.mean = true;
        } else fused.set(m.id, { m, rrf: 1 / (60 + rank) * 0.9, kw: false, mean: true });
      });
    }
  }

  const hits = [...fused.values()]
    .sort((a, b) => b.rrf - a.rrf)
    .slice(0, limit)
    .map(({ m, rrf, kw, mean }) =>
      hit(m, kw ? snippet(m.text ?? "", words, phrases) : plainSnippet(m), kw && mean ? "both" : kw ? "keyword" : "meaning", rrf),
    );
  return { hits, query: q, semantic: semanticReady, tookMs: performance.now() - t0 };
}
