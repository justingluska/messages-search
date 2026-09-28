# Messages Search: architecture (v1)

Working name. Fast, local, read-only search over the macOS Messages history. It is the base for a fuller Messages client later.

```
┌──────────────────────── Tauri 2 app (macOS) ────────────────────────────┐
│  React + TS UI (apps/desktop/src): search · conversation · ask · setup   │
│    talks ONLY through src/lib/api.ts ──invoke()/events──┐                │
│  src-tauri: thin command glue + background indexer      ▼                │
│      ms-source ──reads──▶ ~/Library/Messages/chat.db (read-only)         │
│      ms-core   ──owns───▶ <app data>/index.db (SQLite WAL + FTS5 + vec)  │
│      ms-embed  ──runs───▶ local ONNX embedding model (downloaded once)   │
└──────────────────────────────────────────────────────────────────────────┘
```

## Rules
- chat.db is **never written**. It is opened read-only, and only by `ms-source`.
- **Speed budget:** every UI interaction reads the local index only. Nothing on the interaction path waits on the embedder or on indexing.
- Fixtures and mock data use **fictional people only** (`.example` emails, 555 numbers).
- `crates/ms-core/src/types.rs` ⇄ `apps/desktop/src/lib/types.ts` stay in lockstep. Serde uses camelCase.

## Crates
- `ms-core`: types, the index store (`store.rs`), the query parser (`query.rs`), hybrid search (`search.rs`), and the `Embedder` trait. No Tauri, no chat.db.
- `ms-source`: reads chat.db through `imessage-database` (GPL-3.0, hence this project's license) and turns rows into `Ingest*` types. Resolves contact names (macOS Contacts).
- `ms-embed`: the `Embedder` implementation. It uses fastembed (ONNX Runtime), and the default model is `bge-small-en-v1.5` (384 dims, quantized).
- `ms-engine`: the app backend, independent of Tauri. It owns the store, the chat.db source and watcher, both embedder instances (one for passages, one for queries, so a search never waits behind a batch), and the indexer thread. src-tauri is a thin layer over it.
- `ms-cli` (`ms`): index, search and benchmark from the terminal (`ms synth 500000` builds a synthetic index for perf work). Used for dev and perf checks.
- `apps/desktop`: the Tauri 2 app.

## Search
- **Keyword:** FTS5 (`unicode61 remove_diacritics 2`, `prefix='2 3'`) with one row per message and bm25 ranking.
- **Meaning:** consecutive messages in a chat are grouped into windows. A new window starts after a 45-minute gap, at about 900 characters, or at 16 messages. Each window is embedded as int8 in sqlite-vec with cosine distance.
- **Fusion:** reciprocal rank fusion (k=60). A semantic window is anchored to its best keyword hit when it has one, otherwise to its most representative message.
- **Query operators:** `from:` `in:`/`with:` `has:link|photo|video|audio|file|attachment` `before:` `after:` `during:` `"phrase"` `-word`.

## Command contract (src-tauri ⇄ src/lib/api.ts)

All commands are async. Errors come back as `{ kind: "permission" | "notFound" | "invalid" | "internal", message }`.

| command | args | returns |
|---|---|---|
| `status` | – | `AppStatus` |
| `search` | `query: string, limit?: number` | `SearchResults` |
| `messages_around` | `messageId, before?: number (default 40), after?: number (default 40)` | `MessageView[]` (oldest first) |
| `messages_page` | `chatId, cursorMs, cursorId, newer: boolean, limit?: number (default 60)` | `MessageView[]` (oldest first) |
| `get_chat` | `chatId` | `ChatSummary \| null` |
| `list_chats` | `limit?, offset?` | `ChatSummary[]` |
| `open_in_messages` | `chatId` | – (opens Messages.app on that conversation) |
| `open_full_disk_access_settings` | – | – |
| `reveal_attachment` | `attachmentId` | – (reveals the file in Finder) |
| `reindex` | – | – (full rebuild from chat.db) |
| `open_contacts_settings` | – | – (System Settings → Privacy & Security → Contacts) |
| `reveal_index` | – | – (shows the data folder: index.db, models/, avatars/) |
| `app_version` | – | `string` |
| `insights` | `year: number \| null` | `Insights` (activity for a local calendar year or all time: per-day counts, hours, weekdays, top people/groups, streaks) |
| `storage_summary` | – | `StorageSummary` (attachment totals by type and top 20 conversations) |
| `list_attachments` | `filter: AttachmentFilter` | `AttachmentPage` (rows + totals over the whole filter) |
| `open_attachment` | `attachmentId` | – (default app) |
| `save_attachment` | `attachmentId` | `string` (copy saved in ~/Downloads, never overwriting) |
| `copy_attachment` | `attachmentId` | – (clipboard: image data + file for images, file otherwise) |
| `trash_attachments` | `ids: number[]` | `TrashResult {trashed, bytes, failed: {id, reason}[]}`: moves files to the Trash (restorable). Refuses anything outside ~/Library/Messages/Attachments. Messages then shows those attachments as unavailable; there is no API to delete them from Messages itself. |

Attachments and contact photos are shown through Tauri's asset protocol (`convertFileSrc(path)`), limited to `~/Library/Messages/Attachments/**` and `$APPDATA/avatars/**`. Contact photos are written by the sync from Contacts thumbnails (`Person.avatar`, `SearchHit.people`, `MessageView.senderAvatar`). Contacts access needs `NSContactsUsageDescription` in `src-tauri/Info.plist`; without it, macOS refuses the request without asking.

### Events (backend → UI)
- `index-progress`, payload `IndexProgress`: emitted during indexing and embedding (throttled to about 5 per second). Embedding progress is overall (embedded windows of all windows), so it never goes backward when a sync interrupts it.
- `index-changed`, no payload: new messages were ingested. Re-run the current search and refresh any open conversation.

### `AppStatus`
```ts
{
  access: "ok" | "needsFullDiskAccess" | "noMessagesDb",
  stats: Stats,               // from ms-core
  progress: IndexProgress | null,
  semantic: "ready" | "downloadingModel" | "embedding" | "unavailable",
  modelId: string | null,
  error: string | null,       // last indexing error (e.g. unreadable database)
  contacts: "authorized" | "denied" | "notDetermined" | "unsupported",
}
```

### `IndexProgress`
```ts
{
  phase: "reading" | "indexing" | "windows" | "downloadingModel" | "embedding" | "idle",
  done: number,
  total: number,              // 0 when unknown
  message: string | null,     // e.g. "Downloading model (34 MB)…"
}
```

The other shapes (`Stats`, `ChatSummary`, `Person`, `MessageView`, `AttachmentView`, `ReactionView`, `SearchHit`, `SearchResults`, `ParsedQuery`, `MatchedBy`, `AttachmentKind`, `MessageKind`, `HasFilter`) are defined in `crates/ms-core/src/types.rs` and `crates/ms-core/src/query.rs`. `SearchHit.snippet` marks each match with U+0002 before it and U+0003 after it.
