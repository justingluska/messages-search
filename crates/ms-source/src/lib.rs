//! Reads the macOS Messages database (chat.db) read-only and converts rows
//! into ms-core ingest types. The only code that touches chat.db.

pub mod contacts;
pub mod sync;
pub mod watch;

use std::path::{Path, PathBuf};

use imessage_database::message_types::variants::{Tapback, TapbackAction, Variant};
use imessage_database::tables::attachment::Attachment;
use imessage_database::tables::capabilities::Capabilities;
use imessage_database::tables::chat::Chat;
use imessage_database::tables::chat_handle::ChatToHandle;
use imessage_database::tables::handle::Handle;
use imessage_database::tables::messages::Message;
use imessage_database::tables::table::{get_connection, Cacheable, Table};
use imessage_database::util::dates::{get_local_time, get_offset};
use imessage_database::util::platform::Platform;
use imessage_database::util::query_context::QueryContext;
use ms_core::types::*;
use rusqlite::Connection;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// macOS privacy (TCC) blocked access: the app needs Full Disk Access.
    #[error("no permission to read the Messages database")]
    NoAccess,
    #[error("no Messages database at {0}")]
    NotFound(PathBuf),
    #[error("{0}")]
    Other(String),
}

impl From<imessage_database::error::table::TableError> for SourceError {
    fn from(e: imessage_database::error::table::TableError) -> Self {
        SourceError::Other(e.to_string())
    }
}

/// `~/Library/Messages/chat.db`.
pub fn default_db_path() -> PathBuf {
    imessage_database::util::dirs::default_db_path()
}

/// Can we read chat.db? Distinguishes "no permission" from "doesn't exist".
pub fn check_access(path: &Path) -> Result<(), SourceError> {
    match std::fs::File::open(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err(SourceError::NoAccess),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // TCC can hide the file itself; if the folder is unreadable too,
            // it's a permission problem rather than a missing database.
            match path.parent().map(std::fs::read_dir) {
                Some(Err(pe)) if pe.kind() == std::io::ErrorKind::PermissionDenied => {
                    Err(SourceError::NoAccess)
                }
                _ => Err(SourceError::NotFound(path.to_path_buf())),
            }
        }
        Err(e) => Err(SourceError::Other(e.to_string())),
    }
}

pub struct Source {
    conn: Connection,
    caps: Capabilities,
    db_path: PathBuf,
    /// Seconds between the unix epoch and Apple's 2001 epoch.
    offset: i64,
}

impl Source {
    /// Open chat.db read-only.
    pub fn open(path: &Path) -> Result<Self, SourceError> {
        check_access(path)?;
        let conn = get_connection(path)?;
        let caps = Capabilities::determine(&conn)?;
        Ok(Source {
            conn,
            caps,
            db_path: path.to_path_buf(),
            offset: get_offset(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.db_path
    }

    /// Total rows in `message` (for progress; includes tapbacks).
    pub fn message_count(&self) -> Result<i64, SourceError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM message", [], |r| r.get(0))
            .map_err(|e| SourceError::Other(e.to_string()))
    }

    /// Highest `message.ROWID`: a cheap change detector.
    pub fn max_rowid(&self) -> Result<i64, SourceError> {
        self.conn
            .query_row("SELECT COALESCE(MAX(ROWID), 0) FROM message", [], |r| {
                r.get(0)
            })
            .map_err(|e| SourceError::Other(e.to_string()))
    }

    /// Earliest date (unix ms) among messages added after `rowid`. ROWIDs
    /// only grow, so this catches rows that arrive late or with old dates
    /// (a phone offline for days, an iCloud backfill, a Recently Deleted
    /// restore), which a date-only cursor would skip.
    pub fn min_date_after_rowid(&self, rowid: i64) -> Result<Option<i64>, SourceError> {
        let raw: Option<i64> = self
            .conn
            .query_row(
                "SELECT MIN(date) FROM message WHERE ROWID > ?1",
                [rowid],
                |r| r.get(0),
            )
            .map_err(|e| SourceError::Other(e.to_string()))?;
        Ok(raw
            .and_then(|d| get_local_time(d, self.offset).ok())
            .map(|t| t.timestamp_millis()))
    }

    pub fn handles(&self) -> Result<Vec<IngestHandle>, SourceError> {
        let mut out = Vec::new();
        Handle::stream(&self.conn, |h: Result<Handle, _>| {
            if let Ok(h) = h {
                out.push(IngestHandle {
                    id: h.rowid as i64,
                    address: h.id,
                    name: None,
                });
            }
            Ok::<(), SourceError>(())
        })?;
        Ok(out)
    }

    pub fn chats(&self) -> Result<Vec<IngestChat>, SourceError> {
        let members = ChatToHandle::cache(&self.conn)?;
        let mut out = Vec::new();
        Chat::stream(&self.conn, |c: Result<Chat, _>| {
            if let Ok(c) = c {
                let participants = members
                    .get(&c.rowid)
                    .map(|s| s.iter().map(|h| *h as i64).collect())
                    .unwrap_or_default();
                out.push(IngestChat {
                    id: c.rowid as i64,
                    identifier: c.chat_identifier.clone(),
                    display_name: c.display_name().map(str::to_string),
                    service: c.service_name.clone(),
                    participants,
                });
            }
            Ok::<(), SourceError>(())
        })?;
        Ok(out)
    }

    /// Stream messages (optionally only those on/after `since_ms`, day
    /// granularity) in batches of `batch`. Returns rows read.
    pub fn messages(
        &self,
        since_ms: Option<i64>,
        batch: usize,
        mut on_batch: impl FnMut(Vec<IngestMessage>) -> Result<(), SourceError>,
    ) -> Result<usize, SourceError> {
        let mut context = QueryContext::default();
        if let Some(ms) = since_ms {
            // QueryContext takes a local date; go back a day to cover time zones.
            let day = ms_to_ymd(ms - 86_400_000);
            context
                .set_start(&day)
                .map_err(|e| SourceError::Other(format!("{e:?}")))?;
        }
        let mut stmt = Message::stream_rows(&self.conn, &self.caps, &context)?;
        let mut buf = Vec::with_capacity(batch);
        let mut read = 0usize;
        for row in Message::rows(&mut stmt, [])? {
            read += 1;
            let Ok(mut m) = row else { continue };
            // Message bodies are parsed from blobs other people send; a
            // parser panic on one crafted message skips that message instead
            // of killing indexing on every launch.
            let converted =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.convert(&mut m)));
            match converted {
                Ok(Some(msg)) => buf.push(msg),
                Ok(None) => {}
                Err(_) => eprintln!("skipped message {} (couldn't parse it)", m.rowid),
            }
            if buf.len() >= batch {
                on_batch(std::mem::take(&mut buf))?;
            }
        }
        if !buf.is_empty() {
            on_batch(buf)?;
        }
        Ok(read)
    }

    fn convert(&self, m: &mut Message) -> Option<IngestMessage> {
        if let Ok(body) = m.parse_body(&self.conn) {
            m.apply_body(body);
        }
        let date_ms = get_local_time(m.date, self.offset).ok()?.timestamp_millis();
        let chat_id = m.chat_id.map(|c| c as i64);
        let handle_id = m.handle_id.filter(|h| *h != 0).map(|h| h as i64);

        let mut out = IngestMessage {
            id: m.rowid as i64,
            guid: m.guid.clone(),
            chat_id,
            handle_id,
            from_me: m.is_from_me(),
            date_ms,
            text: None,
            kind: MessageKind::Text,
            service: m.service.clone(),
            reply_to_guid: m.thread_originator_guid.clone(),
            edited: m.is_edited(),
            unsent: m.is_fully_unsent(),
            attachments: Vec::new(),
            reaction: None,
        };

        match m.variant() {
            Variant::Tapback(part, action, tapback) => {
                let (_, target) = m.clean_associated_guid()?;
                let emoji = match tapback {
                    Tapback::Loved => "❤️".to_string(),
                    Tapback::Liked => "👍".to_string(),
                    Tapback::Disliked => "👎".to_string(),
                    Tapback::Laughed => "😂".to_string(),
                    Tapback::Emphasized => "‼️".to_string(),
                    Tapback::Questioned => "❓".to_string(),
                    Tapback::Emoji(e) => e?.to_string(),
                    // Sticker reactions have no emoji to show.
                    Tapback::Sticker => return None,
                };
                out.reaction = Some(IngestReaction {
                    target_guid: target.to_string(),
                    part: part as i64,
                    emoji,
                    removed: matches!(action, TapbackAction::Removed),
                });
                return Some(out);
            }
            Variant::Vote | Variant::PollUpdate | Variant::SharePlay | Variant::Unknown(_) => {
                out.kind = MessageKind::App;
            }
            Variant::App(_) if !m.is_url() => out.kind = MessageKind::App,
            _ => {}
        }
        if m.is_announcement() {
            out.kind = MessageKind::System;
            out.text = m.group_title.clone();
            return chat_id.map(|_| out);
        }

        out.text = if out.unsent { None } else { m.text.clone() }
            // Attachment placeholders (U+FFFC) aren't text.
            .map(|t| t.replace('\u{FFFC}', "").trim().to_string())
            .filter(|t| !t.is_empty());

        if m.has_attachments() {
            if let Ok(atts) = Attachment::from_message(&self.conn, m, &self.caps) {
                for a in atts {
                    if a.hide_attachment != 0 {
                        continue;
                    }
                    let path = a.resolved_attachment_path(&Platform::macOS, &self.db_path, None);
                    let filename = a
                        .transfer_name
                        .clone()
                        .or_else(|| a.filename().map(str::to_string));
                    let kind = AttachmentKind::classify(
                        a.mime_type.as_deref(),
                        filename.as_deref(),
                        a.is_sticker,
                    );
                    out.attachments.push(IngestAttachment {
                        id: a.rowid as i64,
                        filename,
                        mime: a.mime_type.clone(),
                        path,
                        bytes: a.total_bytes,
                        kind,
                    });
                }
            }
        }
        // Messages outside any conversation can't be shown in context.
        chat_id.map(|_| out)
    }
}

/// Unix ms → `YYYY-MM-DD` (UTC is fine: callers already subtract a day).
fn ms_to_ymd(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ymd() {
        assert_eq!(ms_to_ymd(0), "1970-01-01");
        assert_eq!(ms_to_ymd(1_709_251_200_000), "2024-03-01");
    }
}
