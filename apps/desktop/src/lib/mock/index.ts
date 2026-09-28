// In-memory mock backend used when running outside Tauri (browser) or with
// VITE_MOCK=1. Same command names and arg shapes as src-tauri/src/commands.rs.
//
// Simulate app states with a URL param:
//   ?state=ready (default) | needsAccess | noDb | indexing
//   &error=<text> sets AppStatus.error (last indexing error)
//   &contacts=denied|notDetermined|unsupported sets AppStatus.contacts
// needsAccess flips to indexing a few seconds after "Open System Settings",
// so the setup screen's re-check can be exercised.

import type { UnlistenFn } from "@tauri-apps/api/event";
import type { AppStatus, ContactsAccess, IndexProgress, MessageView, SemanticState, Stats } from "../types";
import { BY_CHAT, BY_ID, CHATS, CHAT_BY_ID, MESSAGES } from "./data";
import { mockSearch } from "./search";
import { mockInsights } from "./insights";
import { mockListAttachments, mockStorageSummary, mockTrash } from "./storage";

type Handler = (args: Record<string, any>) => unknown;

const params = new URLSearchParams(typeof location !== "undefined" ? location.search : "");
type MockState = "ready" | "needsAccess" | "noDb" | "indexing";
let state: MockState = (["needsAccess", "noDb", "indexing"].includes(params.get("state") ?? "") ? params.get("state") : "ready") as MockState;
let progress: IndexProgress | null = null;
let semantic: SemanticState = "ready";
let contacts: ContactsAccess = (["denied", "notDetermined", "unsupported"].includes(params.get("contacts") ?? "")
  ? params.get("contacts")
  : "authorized") as ContactsAccess;

const listeners = new Map<string, Set<(p: unknown) => void>>();
function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((cb) => cb(payload));
}

function stats(): Stats {
  const indexed = state === "indexing" && progress?.phase === "indexing" ? progress.done : MESSAGES.length;
  const windows = Math.round(MESSAGES.length / 7);
  return {
    messages: indexed,
    chats: CHATS.length,
    attachments: MESSAGES.reduce((n, m) => n + m.attachments.length, 0),
    windows,
    embeddedWindows: semantic === "ready" ? windows : progress?.phase === "embedding" ? Math.round((windows * progress.done) / Math.max(1, progress.total)) : 0,
    oldestMs: MESSAGES[0]?.dateMs ?? null,
    newestMs: MESSAGES[MESSAGES.length - 1]?.dateMs ?? null,
    indexBytes: 48_300_000,
  };
}

/** A first-run index: reading → indexing → windows → model download → embedding → idle. */
function simulateIndexing() {
  state = "indexing";
  semantic = "unavailable";
  const total = 482_113;
  const steps: IndexProgress[] = [{ phase: "reading", done: 0, total: 0, message: null }];
  for (let i = 1; i <= 120; i++) steps.push({ phase: "indexing", done: Math.round((total * i) / 120), total, message: null });
  for (let i = 1; i <= 10; i++) steps.push({ phase: "windows", done: i * 6800, total: 68_000, message: null });
  for (let i = 1; i <= 40; i++) steps.push({ phase: "downloadingModel", done: i * 850_000, total: 34_000_000, message: "Downloading search model (34 MB)" });
  for (let i = 1; i <= 100; i++) steps.push({ phase: "embedding", done: i * 680, total: 68_000, message: null });
  let k = 0;
  const tick = () => {
    if (k >= steps.length) {
      progress = null;
      semantic = "ready";
      state = "ready";
      emit("index-progress", { phase: "idle", done: 0, total: 0, message: null } satisfies IndexProgress);
      emit("index-changed", null);
      return;
    }
    progress = steps[k++];
    semantic = progress.phase === "downloadingModel" ? "downloadingModel" : progress.phase === "embedding" ? "embedding" : "unavailable";
    emit("index-progress", progress);
    setTimeout(tick, 200);
  };
  tick();
}

if (state === "indexing") simulateIndexing();

function around(messageId: number, before = 40, after = 40): MessageView[] {
  const i = BY_ID.get(messageId);
  if (i === undefined) throw { kind: "notFound", message: `message ${messageId}` };
  const m = MESSAGES[i];
  const list = BY_CHAT.get(m.chatId!)!;
  const j = list.indexOf(m);
  return list.slice(Math.max(0, j - before), j + after + 1);
}

function page(chatId: number, cursorMs: number, cursorId: number, newer: boolean, limit = 60): MessageView[] {
  const list = BY_CHAT.get(chatId);
  if (!list) throw { kind: "notFound", message: `chat ${chatId}` };
  const after = (m: MessageView) => m.dateMs > cursorMs || (m.dateMs === cursorMs && m.id > cursorId);
  const before = (m: MessageView) => m.dateMs < cursorMs || (m.dateMs === cursorMs && m.id < cursorId);
  if (newer) {
    const start = list.findIndex(after);
    return start < 0 ? [] : list.slice(start, start + limit);
  }
  let end = list.length;
  while (end > 0 && !before(list[end - 1])) end--;
  return list.slice(Math.max(0, end - limit), end);
}

const handlers: Record<string, Handler> = {
  status: (): AppStatus => ({
    access: state === "needsAccess" ? "needsFullDiskAccess" : state === "noDb" ? "noMessagesDb" : "ok",
    stats: stats(),
    progress,
    semantic,
    modelId: semantic === "ready" ? "bge-small-en-v1.5-q" : null,
    error: params.get("error"),
    contacts,
  }),
  search: ({ query, limit }) => mockSearch(String(query ?? ""), limit ?? 200, semantic === "ready"),
  messages_around: ({ messageId, before, after }) => around(messageId, before ?? 40, after ?? 40),
  messages_page: ({ chatId, cursorMs, cursorId, newer, limit }) => page(chatId, cursorMs, cursorId, newer, limit ?? 60),
  get_chat: ({ chatId }) => CHAT_BY_ID.get(chatId) ?? null,
  list_chats: ({ limit, offset }) => CHATS.slice(offset ?? 0, (offset ?? 0) + (limit ?? 500)),
  open_in_messages: ({ chatId }) => console.info("mock: open in Messages, chat", chatId),
  open_full_disk_access_settings: () => {
    console.info("mock: open System Settings → Full Disk Access");
    if (state === "needsAccess") setTimeout(simulateIndexing, 3000);
  },
  reveal_attachment: ({ attachmentId }) => console.info("mock: reveal attachment", attachmentId),
  reindex: () => simulateIndexing(),
  reveal_index: () => console.info("mock: reveal index folder"),
  open_contacts_settings: () => {
    console.info("mock: open System Settings, Contacts");
    if (contacts !== "authorized") setTimeout(() => (contacts = "authorized"), 2500);
  },
  app_version: () => "0.1.0",
  insights: ({ year }) => mockInsights(year ?? null),
  storage_summary: () => mockStorageSummary(),
  list_attachments: ({ filter }) => mockListAttachments(filter),
  trash_attachments: ({ ids }) => mockTrash(ids),
  copy_attachment: ({ attachmentId }) => console.info("mock: copy attachment", attachmentId),
  save_attachment: ({ attachmentId }) => `/Users/you/Downloads/attachment-${attachmentId}`,
  open_attachment: ({ attachmentId }) => console.info("mock: open attachment", attachmentId),
  open_storage_settings: () => console.info("mock: open System Settings, Storage, Messages"),
};

export const mockBackend = {
  async invoke<T>(cmd: string, args: Record<string, any>): Promise<T> {
    const h = handlers[cmd];
    if (!h) throw { kind: "internal", message: `mock: no handler for command "${cmd}"` };
    // Simulate the IPC hop.
    await new Promise((r) => setTimeout(r, 3));
    return h(args) as T;
  },
  async listen(event: string, cb: (p: unknown) => void): Promise<UnlistenFn> {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event)!.add(cb);
    return () => listeners.get(event)?.delete(cb);
  },
};
