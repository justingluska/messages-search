// Dates, sizes and names formatted the way Messages shows them.

const time = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const weekday = new Intl.DateTimeFormat(undefined, { weekday: "long" });
const shortWeekday = new Intl.DateTimeFormat(undefined, { weekday: "short", month: "short", day: "numeric" });
const numericDate = new Intl.DateTimeFormat(undefined, { month: "numeric", day: "numeric", year: "2-digit" });
const longDate = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", year: "numeric" });
const num = new Intl.NumberFormat();

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function daysAgo(ms: number, now = Date.now()): number {
  return Math.round((startOfDay(now) - startOfDay(ms)) / 86_400_000);
}

/** Result list date, like the Messages sidebar: "9:41 AM", "Yesterday", "Tuesday", "6/4/24". */
export function listDate(ms: number): string {
  const d = daysAgo(ms);
  if (d <= 0) return time.format(ms);
  if (d === 1) return "Yesterday";
  if (d < 7) return weekday.format(ms);
  return numericDate.format(ms);
}

/** Centered transcript timestamp: "Today 9:41 AM", "Tuesday 4:12 PM", "Sat, Jun 1 at 4:12 PM", "Jun 1, 2024 at 4:12 PM". */
export function transcriptDate(ms: number): string {
  const d = daysAgo(ms);
  const t = time.format(ms);
  if (d <= 0) return `Today ${t}`;
  if (d === 1) return `Yesterday ${t}`;
  if (d < 7) return `${weekday.format(ms)} ${t}`;
  if (new Date(ms).getFullYear() === new Date().getFullYear()) return `${shortWeekday.format(ms)} at ${t}`;
  return `${longDate.format(ms)} at ${t}`;
}

export function fullDate(ms: number): string {
  return `${longDate.format(ms)} at ${time.format(ms)}`;
}

export function formatBytes(n: number): string {
  if (n < 1000) return `${n} bytes`;
  const units = ["KB", "MB", "GB"];
  let v = n / 1000;
  let u = 0;
  while (v >= 1000 && u < units.length - 1) {
    v /= 1000;
    u++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[u]}`;
}

export const formatCount = (n: number) => num.format(n);

export function initials(name: string): string {
  // Phone numbers and addresses get the person glyph, not a digit.
  if (/^[+\d(]/.test(name.trim()) || name.includes("@")) return "";
  const letters = name
    .replace(/[^\p{L}\p{N}\s]/gu, "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (letters.length === 0) return "";
  const first = letters[0][0] ?? "";
  const last = letters.length > 1 ? letters[letters.length - 1][0] : "";
  return (first + last).toUpperCase();
}

/** First name for group transcripts ("Maya Patel" → "Maya"); addresses stay whole. */
export function shortName(name: string): string {
  if (/[@+\d]/.test(name[0] ?? "") || name.includes("@")) return name;
  return name.split(/\s+/)[0];
}
