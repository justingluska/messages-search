//! Types shared by the source reader, the index and the UI. Everything the UI
//! sees serializes camelCase; keep `apps/desktop/src/lib/types.ts` in lockstep.

use serde::{Deserialize, Serialize};

/// What a row in the index is. Only `Text` rows are searchable and chunked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageKind {
    /// A normal message: text and/or attachments.
    Text,
    /// Group events: renamed, member added/left, and similar announcements.
    System,
    /// App/balloon messages without searchable text (Apple Pay, games, ...).
    App,
}

impl MessageKind {
    pub(crate) fn code(self) -> i64 {
        match self {
            MessageKind::Text => 0,
            MessageKind::System => 1,
            MessageKind::App => 2,
        }
    }

    pub(crate) fn from_code(code: i64) -> Self {
        match code {
            1 => MessageKind::System,
            2 => MessageKind::App,
            _ => MessageKind::Text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentKind {
    Image,
    Video,
    Audio,
    Sticker,
    File,
}

impl AttachmentKind {
    pub(crate) fn code(self) -> i64 {
        match self {
            AttachmentKind::Image => 0,
            AttachmentKind::Video => 1,
            AttachmentKind::Audio => 2,
            AttachmentKind::Sticker => 3,
            AttachmentKind::File => 4,
        }
    }

    pub(crate) fn from_code(code: i64) -> Self {
        match code {
            0 => AttachmentKind::Image,
            1 => AttachmentKind::Video,
            2 => AttachmentKind::Audio,
            3 => AttachmentKind::Sticker,
            _ => AttachmentKind::File,
        }
    }

    /// Classify from a MIME type (or UTI-ish fallback on the file name).
    pub fn classify(mime: Option<&str>, filename: Option<&str>, is_sticker: bool) -> Self {
        if is_sticker {
            return AttachmentKind::Sticker;
        }
        let mime = mime.unwrap_or_default();
        if mime.starts_with("image/") {
            return AttachmentKind::Image;
        }
        if mime.starts_with("video/") {
            return AttachmentKind::Video;
        }
        if mime.starts_with("audio/") {
            return AttachmentKind::Audio;
        }
        let ext = filename
            .and_then(|f| f.rsplit_once('.'))
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "heic" | "jpg" | "jpeg" | "png" | "gif" | "webp" | "tiff" => AttachmentKind::Image,
            "mov" | "mp4" | "m4v" => AttachmentKind::Video,
            "caf" | "m4a" | "mp3" | "amr" | "wav" | "aac" => AttachmentKind::Audio,
            _ => AttachmentKind::File,
        }
    }
}

// ---------------------------------------------------------------- ingest ---

/// A person/number/email as Messages knows it (`handle` table).
#[derive(Debug, Clone)]
pub struct IngestHandle {
    /// `handle.ROWID` in chat.db.
    pub id: i64,
    /// Phone number or email address.
    pub address: String,
    /// Contact name, when Contacts resolved it.
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IngestChat {
    /// `chat.ROWID` in chat.db.
    pub id: i64,
    pub identifier: String,
    /// The group name, when the chat has one.
    pub display_name: Option<String>,
    pub service: Option<String>,
    pub participants: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct IngestAttachment {
    /// `attachment.ROWID` in chat.db.
    pub id: i64,
    pub filename: Option<String>,
    pub mime: Option<String>,
    /// Absolute path on disk (with `~` expanded), when the file exists locally.
    pub path: Option<String>,
    pub bytes: i64,
    pub kind: AttachmentKind,
}

/// A tapback (or its removal) pointing at another message.
#[derive(Debug, Clone)]
pub struct IngestReaction {
    pub target_guid: String,
    /// Which part (bubble) of the target message.
    pub part: i64,
    /// Emoji to display ("❤️", "👍", "😂", a custom emoji, ...).
    pub emoji: String,
    /// True for "removed a reaction" rows.
    pub removed: bool,
}

#[derive(Debug, Clone)]
pub struct IngestMessage {
    /// `message.ROWID` in chat.db.
    pub id: i64,
    pub guid: String,
    pub chat_id: Option<i64>,
    pub handle_id: Option<i64>,
    pub from_me: bool,
    /// Unix milliseconds.
    pub date_ms: i64,
    pub text: Option<String>,
    pub kind: MessageKind,
    pub service: Option<String>,
    /// GUID of the message this one replies to (threaded reply).
    pub reply_to_guid: Option<String>,
    pub edited: bool,
    pub unsent: bool,
    pub attachments: Vec<IngestAttachment>,
    /// Set when this row is a tapback rather than a message.
    pub reaction: Option<IngestReaction>,
}

// ---------------------------------------------------------------- output ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub messages: i64,
    pub chats: i64,
    pub attachments: i64,
    pub windows: i64,
    pub embedded_windows: i64,
    pub oldest_ms: Option<i64>,
    pub newest_ms: Option<i64>,
    /// Size of the index file(s) on disk.
    pub index_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub id: i64,
    /// Group name, else the participants' names joined.
    pub title: String,
    pub is_group: bool,
    pub participants: Vec<Person>,
    pub last_ms: Option<i64>,
    pub last_text: Option<String>,
    pub message_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub handle_id: i64,
    pub address: String,
    pub name: Option<String>,
    /// Contact photo file, when Contacts has one.
    pub avatar: Option<String>,
}

impl Person {
    pub fn display(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.address)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentView {
    pub id: i64,
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub path: Option<String>,
    pub bytes: i64,
    pub kind: AttachmentKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReactionView {
    pub emoji: String,
    pub from_me: bool,
    /// Who reacted (display name), None when it's me.
    pub sender: Option<String>,
    pub part: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: i64,
    pub guid: String,
    pub chat_id: Option<i64>,
    pub from_me: bool,
    /// Sender display name (None when it's me).
    pub sender: Option<String>,
    pub sender_handle_id: Option<i64>,
    /// Sender's contact photo file (None when it's me or there's no photo).
    pub sender_avatar: Option<String>,
    pub date_ms: i64,
    pub text: Option<String>,
    pub kind: MessageKind,
    pub service: Option<String>,
    pub reply_to_guid: Option<String>,
    pub edited: bool,
    pub unsent: bool,
    pub attachments: Vec<AttachmentView>,
    pub reactions: Vec<ReactionView>,
}

/// How a search result was found.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MatchedBy {
    Keyword,
    Meaning,
    Both,
    /// Filter-only query (no text): listed newest first.
    Filter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub message_id: i64,
    pub chat_id: Option<i64>,
    pub chat_title: String,
    pub is_group: bool,
    pub from_me: bool,
    pub sender: Option<String>,
    pub date_ms: i64,
    /// Text with matches wrapped in U+0002 … U+0003.
    pub snippet: String,
    pub matched_by: MatchedBy,
    pub attachment_count: i64,
    pub score: f64,
    /// The conversation's other participants (up to 4), for avatars.
    pub people: Vec<Person>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    /// Filters that were understood, echoed back for the UI chips.
    pub query: crate::query::ParsedQuery,
    /// False when meaning search wasn't available (no model / not indexed yet).
    pub semantic: bool,
    pub took_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IndexPhase {
    Reading,
    Indexing,
    Windows,
    DownloadingModel,
    Embedding,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub phase: IndexPhase,
    pub done: u64,
    /// 0 when unknown.
    pub total: u64,
    pub message: Option<String>,
}

impl IndexProgress {
    pub fn new(phase: IndexPhase, done: u64, total: u64) -> Self {
        IndexProgress {
            phase,
            done,
            total,
            message: None,
        }
    }
}

/// Text embedding model. Implemented by `ms-embed`; tests use a fake.
pub trait Embedder: Send + Sync {
    /// Stable model id; changing it invalidates stored vectors.
    fn model_id(&self) -> &str;
    fn dims(&self) -> usize;
    /// Embed stored passages (conversation windows). Vectors must be L2-normalized.
    fn embed_passages(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
    /// Embed a search query. Vector must be L2-normalized.
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String>;
    /// Cosine distance beyond which a window is unrelated to the query.
    /// Model-specific: similarity scales differ a lot between models.
    fn max_distance(&self) -> f64 {
        0.55
    }
}

// -------------------------------------------------------------- insights ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayCount {
    /// Local date, `YYYY-MM-DD`.
    pub day: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersonStat {
    pub person: Person,
    /// Their 1:1 conversation.
    pub chat_id: Option<i64>,
    pub sent: i64,
    pub received: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GroupStat {
    pub chat_id: i64,
    pub title: String,
    pub total: i64,
    pub people: Vec<Person>,
}

/// Activity over one year (or all time), GitHub-contributions style.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    pub year: Option<i32>,
    /// Years that have messages, oldest first.
    pub years: Vec<i32>,
    pub total_messages: i64,
    pub sent: i64,
    pub received: i64,
    pub chats: i64,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    /// Days with at least one message, in order.
    pub days: Vec<DayCount>,
    pub by_hour: Vec<i64>,
    /// 0 = Sunday.
    pub by_weekday: Vec<i64>,
    pub top_people: Vec<PersonStat>,
    pub top_groups: Vec<GroupStat>,
    pub longest_streak: i64,
    pub current_streak: i64,
    pub busiest_day: Option<DayCount>,
    pub attachments_bytes: i64,
    pub attachments_count: i64,
}

// --------------------------------------------------------------- storage ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KindTotal {
    pub kind: AttachmentKind,
    pub count: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatStorage {
    pub chat_id: i64,
    pub title: String,
    pub people: Vec<Person>,
    pub count: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StorageSummary {
    /// All attachments, including those only in iCloud.
    pub total_bytes: i64,
    /// Attachments whose files are actually on this Mac.
    pub bytes_on_disk: i64,
    pub total_count: i64,
    pub by_kind: Vec<KindTotal>,
    /// Top 20 conversations by attachment size.
    pub by_chat: Vec<ChatStorage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentSort {
    Size,
    Date,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentFilter {
    pub kind: Option<AttachmentKind>,
    pub min_bytes: Option<i64>,
    pub chat_id: Option<i64>,
    pub sort: AttachmentSort,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRow {
    pub id: i64,
    pub message_id: i64,
    pub chat_id: Option<i64>,
    pub chat_title: String,
    pub from_me: bool,
    pub sender: Option<String>,
    pub date_ms: i64,
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub path: Option<String>,
    pub bytes: i64,
    pub kind: AttachmentKind,
    /// The file exists locally (not iCloud-only, not trashed).
    pub on_disk: bool,
    /// Moved to the Trash by this app and still there (not emptied).
    pub in_trash: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentPage {
    pub rows: Vec<AttachmentRow>,
    /// Totals over everything matching the filter (not just this page).
    pub total_count: i64,
    pub total_bytes: i64,
}
