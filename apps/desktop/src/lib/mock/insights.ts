// Mock `insights`: computed from the generated history, same shapes as the backend.

import type { DayCount, Insights, TopGroup, TopPerson } from "../types";
import { CHAT_BY_ID, MESSAGES } from "./data";

const pad = (n: number) => String(n).padStart(2, "0");
const dayKey = (ms: number) => {
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

function streaks(days: Map<string, number>, lastMs: number | null): { longest: number; current: number } {
  const keys = [...days.keys()].sort();
  let longest = 0;
  let run = 0;
  let prev: number | null = null;
  for (const k of keys) {
    const t = new Date(`${k}T12:00:00`).getTime();
    run = prev !== null && Math.round((t - prev) / 86_400_000) === 1 ? run + 1 : 1;
    longest = Math.max(longest, run);
    prev = t;
  }
  // Current: consecutive days ending today (or yesterday) with messages.
  let current = 0;
  if (lastMs !== null) {
    const d = new Date();
    d.setHours(12, 0, 0, 0);
    if (!days.has(dayKey(d.getTime()))) d.setDate(d.getDate() - 1);
    while (days.has(dayKey(d.getTime()))) {
      current++;
      d.setDate(d.getDate() - 1);
    }
  }
  return { longest, current };
}

export function mockInsights(year: number | null): Insights {
  const all = MESSAGES.filter((m) => m.kind === "text" && !m.unsent);
  const years = [...new Set(all.map((m) => new Date(m.dateMs).getFullYear()))].sort((a, b) => b - a);
  const msgs = year === null ? all : all.filter((m) => new Date(m.dateMs).getFullYear() === year);

  const byDay = new Map<string, number>();
  const byHour = new Array(24).fill(0);
  const byWeekday = new Array(7).fill(0);
  const perChat = new Map<number, { sent: number; received: number }>();
  let sent = 0;
  let attachmentsBytes = 0;
  let attachmentsCount = 0;
  for (const m of msgs) {
    const d = new Date(m.dateMs);
    const k = dayKey(m.dateMs);
    byDay.set(k, (byDay.get(k) ?? 0) + 1);
    byHour[d.getHours()]++;
    byWeekday[d.getDay()]++;
    if (m.fromMe) sent++;
    const c = perChat.get(m.chatId!) ?? { sent: 0, received: 0 };
    if (m.fromMe) c.sent++;
    else c.received++;
    perChat.set(m.chatId!, c);
    for (const a of m.attachments) {
      attachmentsBytes += a.bytes;
      attachmentsCount++;
    }
  }

  const days: DayCount[] = [...byDay.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([day, count]) => ({ day, count }));
  const busiestDay = days.reduce<DayCount | null>((b, d) => (!b || d.count > b.count ? d : b), null);
  const firstMs = msgs[0]?.dateMs ?? null;
  const lastMs = msgs[msgs.length - 1]?.dateMs ?? null;
  const { longest, current } = streaks(byDay, lastMs);

  const topPeople: TopPerson[] = [];
  const topGroups: TopGroup[] = [];
  for (const [chatId, c] of perChat) {
    const chat = CHAT_BY_ID.get(chatId);
    if (!chat) continue;
    if (chat.isGroup) topGroups.push({ chatId, title: chat.title, total: c.sent + c.received, people: chat.participants.slice(0, 4) });
    else if (chat.participants[0]) topPeople.push({ person: chat.participants[0], chatId, sent: c.sent, received: c.received, total: c.sent + c.received });
  }
  topPeople.sort((a, b) => b.total - a.total);
  topGroups.sort((a, b) => b.total - a.total);

  return {
    year,
    years,
    totalMessages: msgs.length,
    sent,
    received: msgs.length - sent,
    chats: perChat.size,
    firstMs,
    lastMs,
    days,
    byHour,
    byWeekday,
    topPeople: topPeople.slice(0, 20),
    topGroups: topGroups.slice(0, 10),
    longestStreak: longest,
    currentStreak: current,
    busiestDay,
    attachmentsBytes,
    attachmentsCount,
  };
}
