// Mirrors crates/ms-core/src/types.rs and crates/ms-core/src/query.rs
// (serde camelCase), plus the app-level AppStatus / IndexProgress from
// apps/desktop/src-tauri/src/status.rs. Keep these in lockstep.

/** Unix milliseconds. */
export type Millis = number;

export type MessageKind = "text" | "system" | "app";
export type AttachmentKind = "image" | "video" | "audio" | "sticker" | "file";
export type HasFilter = "link" | "photo" | "video" | "audio" | "file" | "attachment";
/** How a search result was found. `filter`: operator-only query, newest first. */
export type MatchedBy = "keyword" | "meaning" | "both" | "filter";

export interface Stats {
  messages: number;
  chats: number;
  attachments: number;
  windows: number;
  embeddedWindows: number;
  oldestMs: Millis | null;
  newestMs: Millis | null;
  /** Size of the index file(s) on disk. */
  indexBytes: number;
}

export interface Person {
  handleId: number;
  /** Phone number or email address. */
  address: string;
  name: string | null;
  /** Absolute path to a contact thumbnail (JPEG); show through attachmentSrc(). */
  avatar: string | null;
}

export interface ChatSummary {
  id: number;
  /** Group name, else the participants' names joined. */
  title: string;
  isGroup: boolean;
  participants: Person[];
  lastMs: Millis | null;
  lastText: string | null;
  messageCount: number;
}

export interface AttachmentView {
  id: number;
  filename: string | null;
  mime: string | null;
  /** Absolute path on disk; show through attachmentSrc() (asset protocol). */
  path: string | null;
  bytes: number;
  kind: AttachmentKind;
}

export interface ReactionView {
  emoji: string;
  fromMe: boolean;
  /** Who reacted (display name), null when it's me. */
  sender: string | null;
  part: number;
}

export interface MessageView {
  id: number;
  guid: string;
  chatId: number | null;
  fromMe: boolean;
  /** Sender display name (null when it's me). */
  sender: string | null;
  senderHandleId: number | null;
  /** Sender's contact thumbnail path, when known. */
  senderAvatar: string | null;
  dateMs: Millis;
  text: string | null;
  kind: MessageKind;
  service: string | null;
  replyToGuid: string | null;
  edited: boolean;
  unsent: boolean;
  attachments: AttachmentView[];
  reactions: ReactionView[];
}

export interface SearchHit {
  messageId: number;
  chatId: number | null;
  chatTitle: string;
  isGroup: boolean;
  fromMe: boolean;
  sender: string | null;
  dateMs: Millis;
  /** Text with matches wrapped in U+0002 … U+0003. */
  snippet: string;
  matchedBy: MatchedBy;
  attachmentCount: number;
  score: number;
  /** The chat's participants other than me (up to 4), for row avatars. */
  people: Person[];
}

export interface ParsedQuery {
  words: string[];
  phrases: string[];
  excluded: string[];
  /** `from:` values; "me" means sent by me. */
  from: string[];
  /** `in:` / `with:` values. */
  chat: string[];
  has: HasFilter[];
  /** Inclusive lower bound, unix ms. */
  afterMs: Millis | null;
  /** Exclusive upper bound, unix ms. */
  beforeMs: Millis | null;
}

export interface SearchResults {
  hits: SearchHit[];
  /** Filters that were understood, echoed back. */
  query: ParsedQuery;
  /** False when meaning search wasn't available (no model / not indexed yet). */
  semantic: boolean;
  tookMs: number;
}

// ------------------------------------------------------------ app status ---

export type Access = "ok" | "needsFullDiskAccess" | "noMessagesDb";
export type SemanticState = "ready" | "downloadingModel" | "embedding" | "unavailable";
export type ContactsAccess = "authorized" | "denied" | "notDetermined" | "unsupported";
export type IndexPhase = "reading" | "indexing" | "windows" | "downloadingModel" | "embedding" | "idle";

/** Payload of the `index-progress` event. */
export interface IndexProgress {
  phase: IndexPhase;
  done: number;
  /** 0 when unknown. */
  total: number;
  message: string | null;
}

export interface AppStatus {
  access: Access;
  stats: Stats;
  progress: IndexProgress | null;
  semantic: SemanticState;
  modelId: string | null;
  /** Last indexing error (e.g. unreadable database), or null. */
  error: string | null;
  /** macOS Contacts permission (names and photos). */
  contacts: ContactsAccess;
}

/** Every command rejects with this shape. */
export type CommandErrorKind = "permission" | "notFound" | "invalid" | "internal";
export interface CommandError {
  kind: CommandErrorKind;
  message: string;
}

// ------------------------------------------------------------- insights ---

export interface DayCount {
  /** Local date, YYYY-MM-DD. */
  day: string;
  count: number;
}

export interface TopPerson {
  person: Person;
  chatId: number | null;
  sent: number;
  received: number;
  total: number;
}

export interface TopGroup {
  chatId: number;
  title: string;
  total: number;
  people: Person[];
}

/** Counts over the selected year, or all time when `year` is null. */
export interface Insights {
  year: number | null;
  years: number[];
  totalMessages: number;
  sent: number;
  received: number;
  chats: number;
  firstMs: Millis | null;
  lastMs: Millis | null;
  days: DayCount[];
  /** 24 entries, local hour. */
  byHour: number[];
  /** 7 entries, 0 = Sunday. */
  byWeekday: number[];
  topPeople: TopPerson[];
  topGroups: TopGroup[];
  longestStreak: number;
  currentStreak: number;
  busiestDay: DayCount | null;
  attachmentsBytes: number;
  attachmentsCount: number;
}

// -------------------------------------------------------------- storage ---

export interface KindStorage {
  kind: AttachmentKind;
  count: number;
  bytes: number;
}

export interface ChatStorage {
  chatId: number;
  title: string;
  people: Person[];
  count: number;
  bytes: number;
}

export interface StorageSummary {
  totalBytes: number;
  /** Sum of attachment files actually on this Mac (not iCloud-only, not in the Trash). */
  bytesOnDisk: number;
  totalCount: number;
  byKind: KindStorage[];
  /** Top 20. */
  byChat: ChatStorage[];
}

export interface AttachmentFilter {
  kind: AttachmentKind | null;
  minBytes: number | null;
  chatId: number | null;
  sort: "size" | "date";
  limit: number;
  offset: number;
}

export interface AttachmentRow {
  id: number;
  messageId: number;
  chatId: number | null;
  chatTitle: string;
  fromMe: boolean;
  sender: string | null;
  dateMs: Millis;
  filename: string | null;
  mime: string | null;
  path: string | null;
  bytes: number;
  kind: AttachmentKind;
  /** False when the file lives only in iCloud. */
  onDisk: boolean;
  /** Moved to the Trash by us and still there (best effort). */
  inTrash: boolean;
}

export interface AttachmentPage {
  rows: AttachmentRow[];
  totalCount: number;
  totalBytes: number;
}

export interface TrashResult {
  trashed: number;
  bytes: number;
  failed: { id: number; reason: string }[];
}
