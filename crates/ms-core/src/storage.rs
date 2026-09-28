//! Attachment storage views (the Storage page).

use rusqlite::params_from_iter;
use rusqlite::types::Value;

use crate::store::{participants, Store};
use crate::types::*;
use crate::Error;

const MAX_PAGE: i64 = 500;

impl Store {
    pub fn storage_summary(&self) -> Result<StorageSummary, Error> {
        let conn = self.reader();
        let by_kind: Vec<KindTotal> = conn
            .prepare_cached("SELECT kind, COUNT(*), COALESCE(SUM(bytes), 0) FROM attachments GROUP BY kind ORDER BY 3 DESC")?
            .query_map([], |r| Ok(KindTotal { kind: AttachmentKind::from_code(r.get(0)?), count: r.get(1)?, bytes: r.get(2)? }))?
            .collect::<Result<_, _>>()?;
        let chats: Vec<(i64, String, i64, i64)> = conn
            .prepare_cached(
                // CROSS JOIN pins the loop order: drive from attachments (15k)
                // rather than probing attachments for every message (300k+).
                "SELECT m.chat_id, COALESCE(c.title, ''), COUNT(*), SUM(a.bytes) FROM attachments a
                 CROSS JOIN messages m ON m.id = a.message_id LEFT JOIN chats c ON c.id = m.chat_id
                 WHERE m.chat_id IS NOT NULL GROUP BY m.chat_id ORDER BY 4 DESC LIMIT 20",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?;
        let mut by_chat = Vec::with_capacity(chats.len());
        for (chat_id, title, count, bytes) in chats {
            let mut people = participants(&conn, chat_id)?;
            people.truncate(4);
            by_chat.push(ChatStorage {
                chat_id,
                title,
                people,
                count,
                bytes,
            });
        }
        let files: Vec<(String, i64)> = conn
            .prepare_cached("SELECT path, bytes FROM attachments WHERE path IS NOT NULL")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        // Don't hold a reader while touching the disk.
        drop(conn);
        // Stat every file: ~15k stats is a few ms, and it's the only truth
        // about what iCloud has offloaded or the user has trashed.
        let bytes_on_disk: i64 = files
            .iter()
            .filter(|(p, _)| std::path::Path::new(p).is_file())
            .map(|(_, b)| *b)
            .sum();
        Ok(StorageSummary {
            bytes_on_disk,
            total_bytes: by_kind.iter().map(|k| k.bytes).sum(),
            total_count: by_kind.iter().map(|k| k.count).sum(),
            by_kind,
            by_chat,
        })
    }

    /// A page of attachments matching `f`, plus totals over all matches.
    pub fn list_attachments(&self, f: &AttachmentFilter) -> Result<AttachmentPage, Error> {
        let conn = self.reader();
        let mut wh = String::from("WHERE 1=1");
        let mut p: Vec<Value> = Vec::new();
        if let Some(k) = f.kind {
            p.push(Value::Integer(k.code()));
            wh.push_str(&format!(" AND a.kind = ?{}", p.len()));
        }
        if let Some(min) = f.min_bytes {
            p.push(Value::Integer(min));
            wh.push_str(&format!(" AND a.bytes >= ?{}", p.len()));
        }
        if let Some(c) = f.chat_id {
            p.push(Value::Integer(c));
            wh.push_str(&format!(" AND m.chat_id = ?{}", p.len()));
        }
        let from = "FROM attachments a JOIN messages m ON m.id = a.message_id";
        let (total_count, total_bytes): (i64, i64) = conn.query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM(a.bytes), 0) {from} {wh}"),
            params_from_iter(p.iter()),
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let order = match f.sort {
            AttachmentSort::Size => "a.bytes DESC, m.date_ms DESC",
            AttachmentSort::Date => "m.date_ms DESC, a.id DESC",
        };
        let limit = f.limit.clamp(1, MAX_PAGE);
        let offset = f.offset.max(0);
        let sql = format!(
            "SELECT a.id, a.message_id, m.chat_id, COALESCE(c.title, ''), m.from_me, COALESCE(h.name, h.address),
                    m.date_ms, a.filename, a.mime, a.path, a.bytes, a.kind, t.trash_path
             {from} LEFT JOIN chats c ON c.id = m.chat_id LEFT JOIN handles h ON h.id = m.handle_id
             LEFT JOIN trashed_attachments t ON t.id = a.id
             {wh} ORDER BY {order} LIMIT {limit} OFFSET {offset}"
        );
        let rows: Vec<AttachmentRow> = conn
            .prepare_cached(&sql)?
            .query_map(params_from_iter(p.iter()), |r| {
                let from_me: bool = r.get(4)?;
                let path: Option<String> = r.get(9)?;
                Ok(AttachmentRow {
                    id: r.get(0)?,
                    message_id: r.get(1)?,
                    chat_id: r.get(2)?,
                    chat_title: r.get(3)?,
                    from_me,
                    sender: if from_me { None } else { r.get(5)? },
                    date_ms: r.get(6)?,
                    filename: r.get(7)?,
                    mime: r.get(8)?,
                    on_disk: path
                        .as_deref()
                        .is_some_and(|p| std::path::Path::new(p).is_file()),
                    path,
                    bytes: r.get(10)?,
                    kind: AttachmentKind::from_code(r.get(11)?),
                    in_trash: r
                        .get::<_, Option<String>>(12)?
                        .is_some_and(|t| std::path::Path::new(&t).exists()),
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(AttachmentPage {
            rows,
            total_count,
            total_bytes,
        })
    }
}
