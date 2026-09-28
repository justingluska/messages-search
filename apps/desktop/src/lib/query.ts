// The search box grammar on the UI side: a port of crates/ms-core/src/query.rs
// tokenizing, used to (1) let the filter row read and edit operators in the
// query string (the string is the source of truth), (2) highlight the typed
// terms inside the open transcript, and (3) power the mock backend's parser.

import type { HasFilter, ParsedQuery } from "./types";

export type Tok =
  | { t: "word"; text: string; start: number; end: number }
  | { t: "phrase"; text: string; start: number; end: number }
  | { t: "op"; key: string; value: string; start: number; end: number };

export function tokenize(input: string): Tok[] {
  const out: Tok[] = [];
  const n = input.length;
  let i = 0;
  const readQuoted = (from: number): [string, number] => {
    let j = from;
    while (j < n && input[j] !== '"') j++;
    return [input.slice(from, j), Math.min(j + 1, n)];
  };
  while (i < n) {
    if (/\s/.test(input[i])) {
      i++;
      continue;
    }
    const start = i;
    if (input[i] === '"') {
      const [s, next] = readQuoted(i + 1);
      out.push({ t: "phrase", text: s, start, end: next });
      i = next;
      continue;
    }
    while (i < n && !/\s/.test(input[i]) && input[i] !== ":" && input[i] !== '"') i++;
    const head = input.slice(start, i);
    const isKey = head.length > 0 && /^[A-Za-z_]+$/.test(head);
    if (i < n && input[i] === ":" && isKey) {
      i++;
      if (i < n && input[i] === '"') {
        const [v, next] = readQuoted(i + 1);
        out.push({ t: "op", key: head.toLowerCase(), value: v, start, end: next });
        i = next;
      } else {
        const vs = i;
        while (i < n && !/\s/.test(input[i])) i++;
        out.push({ t: "op", key: head.toLowerCase(), value: input.slice(vs, i), start, end: i });
      }
      continue;
    }
    while (i < n && !/\s/.test(input[i])) i++;
    out.push({ t: "word", text: input.slice(start, i), start, end: i });
  }
  return out;
}

const HAS: Record<string, HasFilter> = {
  link: "link", links: "link", url: "link",
  photo: "photo", photos: "photo", image: "photo", images: "photo", pic: "photo", picture: "photo",
  video: "video", videos: "video",
  audio: "audio", voice: "audio", voicememo: "audio",
  file: "file", files: "file", doc: "file", pdf: "file",
  attachment: "attachment", attachments: "attachment",
};

export function hasFilterOf(value: string): HasFilter | null {
  return HAS[value.toLowerCase()] ?? null;
}

/** `YYYY`, `YYYY-MM`, `YYYY-MM-DD` → [start, end) local time, unix ms. */
export function dateRange(s: string): [number, number] | null {
  const parts = s.split(/[-/]/);
  const y = Number(parts[0]);
  if (!Number.isInteger(y) || y < 1990 || y > 2200) return null;
  if (parts.length === 1) return [new Date(y, 0, 1).getTime(), new Date(y + 1, 0, 1).getTime()];
  const m = Number(parts[1]);
  if (!Number.isInteger(m) || m < 1 || m > 12) return null;
  if (parts.length === 2) return [new Date(y, m - 1, 1).getTime(), new Date(y, m, 1).getTime()];
  const d = Number(parts[2]);
  if (parts.length !== 3 || !Number.isInteger(d) || d < 1 || d > 31) return null;
  return [new Date(y, m - 1, d).getTime(), new Date(y, m - 1, d + 1).getTime()];
}

const OP_KEYS = new Set(["from", "in", "with", "to", "has", "before", "after", "during", "on", "in_year"]);

/** Whether this operator token is one the backend understands (else it's searched as text). */
export function isKnownOp(tok: Extract<Tok, { t: "op" }>): boolean {
  if (!OP_KEYS.has(tok.key) || !tok.value.trim()) return false;
  if (tok.key === "has") return hasFilterOf(tok.value) !== null;
  if (["before", "after", "during", "on", "in_year"].includes(tok.key)) return dateRange(tok.value) !== null;
  return true;
}

export function parseQuery(input: string): ParsedQuery {
  const q: ParsedQuery = { words: [], phrases: [], excluded: [], from: [], chat: [], has: [], afterMs: null, beforeMs: null };
  const word = (w: string) => {
    if (w.startsWith("-")) {
      if (w.length > 1) q.excluded.push(w.slice(1));
    } else q.words.push(w);
  };
  for (const tok of tokenize(input)) {
    if (tok.t === "phrase") {
      if (tok.text.trim()) q.phrases.push(tok.text);
    } else if (tok.t === "word") word(tok.text);
    else if (!isKnownOp(tok)) word(`${tok.key}:${tok.value}`);
    else {
      const v = tok.value.trim();
      switch (tok.key) {
        case "from":
          q.from.push(v);
          break;
        case "in":
        case "with":
        case "to":
          q.chat.push(v);
          break;
        case "has": {
          const f = hasFilterOf(v)!;
          if (!q.has.includes(f)) q.has.push(f);
          break;
        }
        case "before":
          q.beforeMs = dateRange(v)![0];
          break;
        case "after":
          q.afterMs = dateRange(v)![1];
          break;
        default: {
          const [s, e] = dateRange(v)!;
          q.afterMs = s;
          q.beforeMs = e;
        }
      }
    }
  }
  return q;
}

// ------------------------------------------------------ editing operators ---

/** Which filter-row group an operator key belongs to. */
export type FilterGroup = "person" | "chat" | "date" | "has";
const GROUP_KEYS: Record<FilterGroup, string[]> = {
  person: ["from"],
  chat: ["in", "with", "to"],
  date: ["before", "after", "during", "on", "in_year"],
  has: ["has"],
};

export function quoteValue(v: string): string {
  return /[\s"]/.test(v) ? `"${v.replace(/"/g, "")}"` : v;
}

/** The known operators of one group currently in the query, in order. */
export function opsIn(input: string, group: FilterGroup): Extract<Tok, { t: "op" }>[] {
  return tokenize(input).filter((t): t is Extract<Tok, { t: "op" }> => t.t === "op" && GROUP_KEYS[group].includes(t.key) && isKnownOp(t));
}

/**
 * Replace every operator of `group` with `ops` (each `key:value`, already
 * quoted). Operators go at the front, free text stays where it was typed.
 */
export function setGroup(input: string, group: FilterGroup, ops: string[]): string {
  const remove = opsIn(input, group);
  let rest = input;
  for (let k = remove.length - 1; k >= 0; k--) rest = rest.slice(0, remove[k].start) + rest.slice(remove[k].end);
  rest = rest.replace(/\s+/g, " ").trim();
  // Keep operators grouped ahead of the free text, in filter-row order.
  const others = tokenize(rest);
  const leadingOps: string[] = [];
  let cut = 0;
  for (const t of others) {
    if (t.t !== "op" || !isKnownOp(t)) break;
    leadingOps.push(rest.slice(t.start, t.end));
    cut = t.end;
  }
  const text = rest.slice(cut).trim();
  const order: FilterGroup[] = ["person", "chat", "date", "has"];
  const groupOf = (s: string): FilterGroup => {
    const key = s.slice(0, s.indexOf(":")).toLowerCase();
    return order.find((g) => GROUP_KEYS[g].includes(key)) ?? "has";
  };
  const all = [...leadingOps, ...ops].sort((a, b) => order.indexOf(groupOf(a)) - order.indexOf(groupOf(b)));
  return [...all, text].filter(Boolean).join(" ") + (text || all.length === 0 ? "" : " ");
}

// ---------------------------------------------------------- highlighting ---

// Words too common to be worth painting yellow in a transcript.
const STOPWORDS = new Set(
  "a an and are at be but by do for from i if in is it me my of on or so that the this to was we were what when where who why will with you your".split(" "),
);

/**
 * Case-insensitive regex for the query's free-text terms (words as prefixes,
 * phrases exactly), or null. Stopwords and one-letter words are skipped
 * unless they're all there is.
 */
export function termsRegex(q: ParsedQuery): RegExp | null {
  const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const parts: string[] = [];
  for (const p of q.phrases) if (p.trim()) parts.push(esc(p.trim()).replace(/\s+/g, "\\s+"));
  const toks = q.words.flatMap((w) => w.split(/[^\p{L}\p{N}]+/u)).filter(Boolean);
  const useful = toks.filter((t) => t.length > 1 && !STOPWORDS.has(t.toLowerCase()));
  for (const tok of useful.length ? useful : toks) parts.push(`(?<![\\p{L}\\p{N}])${esc(tok)}`);
  if (parts.length === 0) return null;
  parts.sort((a, b) => b.length - a.length);
  return new RegExp(`(${parts.join("|")})`, "giu");
}
