// The ONLY way the UI talks to the Rust backend. Every function maps 1:1 to
// a #[tauri::command] in src-tauri/src/commands.rs (snake_case name, camelCase
// args). Outside Tauri (plain `vite` in a browser) or with VITE_MOCK=1, calls
// go to the in-memory mock backend so the UI can be built and reviewed alone.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppStatus,
  AttachmentFilter,
  AttachmentPage,
  ChatSummary,
  CommandError,
  IndexProgress,
  Insights,
  MessageView,
  SearchResults,
  StorageSummary,
  TrashResult,
} from "./types";

export const EVENTS = {
  indexProgress: "index-progress",
  indexChanged: "index-changed",
} as const;

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
// The mock backend exists only in dev builds (or VITE_MOCK=1); in a production
// build `mockAllowed` is a constant false, so the mock module is dropped from
// the bundle entirely.
const mockAllowed = import.meta.env.DEV || import.meta.env.VITE_MOCK === "1";
export const isMock = mockAllowed && (import.meta.env.VITE_MOCK === "1" || !inTauri);

let mockModule: Promise<typeof import("./mock")> | null = null;
function mockBackend() {
  mockModule ??= import("./mock");
  return mockModule.then((m) => m.mockBackend);
}

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return isMock ? mockBackend().then((m) => m.invoke<T>(cmd, args ?? {})) : invoke<T>(cmd, args);
}

/** Normalize anything a command rejected with into `{ kind, message }`. */
export function asCommandError(e: unknown): CommandError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) return e as CommandError;
  return { kind: "internal", message: e instanceof Error ? e.message : String(e) };
}

export const api = {
  status: () => call<AppStatus>("status"),
  /** `query` is the raw search box text, operators included. */
  search: (query: string, limit?: number) => call<SearchResults>("search", { query, limit: limit ?? null }),
  /** Oldest first; the hit is included. Defaults: 40 before, 40 after. */
  messagesAround: (messageId: number, before?: number, after?: number) =>
    call<MessageView[]>("messages_around", { messageId, before: before ?? null, after: after ?? null }),
  /** Messages strictly older (newer=false) or newer than the cursor message; oldest first. */
  messagesPage: (chatId: number, cursorMs: number, cursorId: number, newer: boolean, limit?: number) =>
    call<MessageView[]>("messages_page", { chatId, cursorMs, cursorId, newer, limit: limit ?? null }),
  getChat: (chatId: number) => call<ChatSummary | null>("get_chat", { chatId }),
  listChats: (limit?: number, offset?: number) => call<ChatSummary[]>("list_chats", { limit: limit ?? null, offset: offset ?? null }),
  openInMessages: (chatId: number) => call<void>("open_in_messages", { chatId }),
  openFullDiskAccessSettings: () => call<void>("open_full_disk_access_settings"),
  revealAttachment: (attachmentId: number) => call<void>("reveal_attachment", { attachmentId }),
  reindex: () => call<void>("reindex"),
  /** Reveal the index folder in Finder. */
  revealIndex: () => call<void>("reveal_index"),
  /** System Settings, Privacy & Security, Contacts. */
  openContactsSettings: () => call<void>("open_contacts_settings"),
  appVersion: () => call<string>("app_version"),

  /** Stats for one year, or all time when `year` is null. */
  insights: (year: number | null) => call<Insights>("insights", { year }),
  storageSummary: () => call<StorageSummary>("storage_summary"),
  listAttachments: (filter: AttachmentFilter) => call<AttachmentPage>("list_attachments", { filter }),
  /** Moves the files to the Trash (restorable); Messages then shows them as unavailable. */
  trashAttachments: (ids: number[]) => call<TrashResult>("trash_attachments", { ids }),
  copyAttachment: (attachmentId: number) => call<void>("copy_attachment", { attachmentId }),
  /** Copies into ~/Downloads; resolves with the saved path. */
  saveAttachment: (attachmentId: number) => call<string>("save_attachment", { attachmentId }),
  /** Opens the file in its default app. */
  openAttachment: (attachmentId: number) => call<void>("open_attachment", { attachmentId }),
  /** System Settings, General, Storage, Messages (Apple's tool that also deletes from iCloud). */
  openStorageSettings: () => call<void>("open_storage_settings"),
};

function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (isMock) return mockBackend().then((m) => m.listen(event, (p) => cb(p as T)));
  return listen<T>(event, (e) => cb(e.payload));
}

/** Indexing / embedding progress (about 5 per second while busy). */
export const onIndexProgress = (cb: (p: IndexProgress) => void) => on<IndexProgress>(EVENTS.indexProgress, cb);
/** New messages were ingested: re-run the search and refresh the open conversation. */
export const onIndexChanged = (cb: () => void) => on<null>(EVENTS.indexChanged, () => cb());

/** An attachment path as an <img>/<video> src (Tauri asset protocol; the mock uses data: URIs). */
export function attachmentSrc(path: string): string {
  if (path.startsWith("data:")) return path;
  return isMock ? path : convertFileSrc(path);
}

/** Open a web link from a message in the default browser. */
export function openLink(url: string): void {
  if (isMock || !inTauri) {
    window.open(url, "_blank", "noopener");
    return;
  }
  import("@tauri-apps/plugin-opener")
    .then((m) => m.openUrl(url))
    // A refused URL (outside the http(s) scope) must not fail silently.
    .catch((err) => console.error("couldn't open link", err));
}
