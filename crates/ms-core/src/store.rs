//! The local index: a SQLite file (WAL) we own, mirroring the searchable parts
//! of chat.db plus an FTS5 index and conversation-window embeddings
//! (sqlite-vec). chat.db itself is only ever opened read-only, by ms-source.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once};

use rusqlite::{params, Connection, OptionalExtension};

use crate::types::*;
use crate::Error;

/// An attachment's (path, filename, kind).
pub type AttachmentFile = (Option<String>, Option<String>, AttachmentKind);

/// Reader connections (see `Store::reader`).
const READERS: usize = 3;

/// Bump when the schema changes incompatibly; the index is rebuilt from chat.db.
const SCHEMA_VERSION: i64 = 2;

/// A new window starts after this much silence in a conversation.
pub const WINDOW_GAP_MS: i64 = 45 * 60 * 1000;
/// ...or once a window holds this much text (~220 tokens) or this many messages.
pub const WINDOW_MAX_CHARS: usize = 900;
pub const WINDOW_MAX_MESSAGES: usize = 16;

pub struct Store {
    path: PathBuf,
    /// Single writer (ingest, windows, embeddings).
    write: Mutex<Connection>,
    /// Readers for the UI; WAL lets them run while the writer works. Several,
    /// so a slow page (Insights) doesn't hold up a search.
    read: Vec<Mutex<Connection>>,
    /// Local UTC offset, for dates in window text.
    tz: crate::tz::Tz,
}

fn register_sqlite_vec() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // SAFETY: sqlite-vec's documented registration for rusqlite; the init
        // function has the sqlite3 extension-entry-point signature.
        #[allow(clippy::missing_transmute_annotations)]
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

fn open_conn(path: &Path) -> Result<Connection, Error> {
    register_sqlite_vec();
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=OFF;
         PRAGMA temp_store=MEMORY;
         PRAGMA cache_size=-32768;
         PRAGMA journal_size_limit=16777216;
         PRAGMA mmap_size=1073741824;",
    )?;
    Ok(conn)
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE IF NOT EXISTS handles(
    id INTEGER PRIMARY KEY,
    address TEXT NOT NULL,
    name TEXT,
    digits TEXT,           -- address with non-digits removed (phone matching)
    avatar TEXT            -- contact photo file (see Store::avatars_dir)
);

CREATE TABLE IF NOT EXISTS chats(
    id INTEGER PRIMARY KEY,
    identifier TEXT NOT NULL,
    display_name TEXT,
    service TEXT,
    is_group INTEGER NOT NULL DEFAULT 0,
    title TEXT NOT NULL DEFAULT '',
    last_ms INTEGER,
    message_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS chat_handles(
    chat_id INTEGER NOT NULL,
    handle_id INTEGER NOT NULL,
    PRIMARY KEY(chat_id, handle_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS chat_handles_handle ON chat_handles(handle_id);

-- `id` is our own, assigned in ingest order. A full sync reads chat.db in
-- date order, so ids (and FTS rowids) are chronological, which lets common
-- words rank only their newest matches (see search.rs).
CREATE TABLE IF NOT EXISTS messages(
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL UNIQUE, -- chat.db message.ROWID
    guid TEXT NOT NULL,
    chat_id INTEGER,
    handle_id INTEGER,
    from_me INTEGER NOT NULL,
    date_ms INTEGER NOT NULL,
    text TEXT,
    kind INTEGER NOT NULL,
    service TEXT,
    reply_to TEXT,
    edited INTEGER NOT NULL DEFAULT 0,
    unsent INTEGER NOT NULL DEFAULT 0,
    has_link INTEGER NOT NULL DEFAULT 0,
    attach_count INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS messages_chat_date ON messages(chat_id, date_ms);
CREATE INDEX IF NOT EXISTS messages_date ON messages(date_ms);
CREATE INDEX IF NOT EXISTS messages_handle_date ON messages(handle_id, date_ms);
-- Partial indexes keep has:link / has:attachment fast even when rare.
CREATE INDEX IF NOT EXISTS messages_link ON messages(date_ms) WHERE has_link = 1;
CREATE INDEX IF NOT EXISTS messages_attach ON messages(date_ms) WHERE attach_count > 0;

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    text,
    content='messages',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE OF text ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO messages_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TABLE IF NOT EXISTS reactions(
    id INTEGER PRIMARY KEY,           -- chat.db ROWID of the tapback row
    target_guid TEXT NOT NULL,
    part INTEGER NOT NULL,
    handle_id INTEGER,
    from_me INTEGER NOT NULL,
    emoji TEXT NOT NULL,
    date_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS reactions_target ON reactions(target_guid);

CREATE TABLE IF NOT EXISTS attachments(
    id INTEGER PRIMARY KEY,           -- chat.db attachment.ROWID
    message_id INTEGER NOT NULL,      -- messages.id
    filename TEXT,
    mime TEXT,
    path TEXT,
    bytes INTEGER NOT NULL DEFAULT 0,
    kind INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS attachments_message ON attachments(message_id);
CREATE INDEX IF NOT EXISTS attachments_kind ON attachments(kind, message_id);
CREATE INDEX IF NOT EXISTS attachments_bytes ON attachments(bytes);

-- Chats whose windows must be rebuilt from `from_ms`. Written in the same
-- transaction as the message changes and cleared by the rebuild, so a crash
-- (or quit) in between can't leave messages without windows forever.
CREATE TABLE IF NOT EXISTS dirty_windows(
    chat_id INTEGER PRIMARY KEY,
    from_ms INTEGER NOT NULL
);

-- Attachments this app moved to the Trash, and where they went.
CREATE TABLE IF NOT EXISTS trashed_attachments(
    id INTEGER PRIMARY KEY,           -- attachments.id
    trash_path TEXT NOT NULL,
    trashed_ms INTEGER NOT NULL
);

-- Consecutive messages in one chat, grouped for embedding: the messages of
-- `chat_id` from (start_ms, first_id) to (end_ms, last_id). The text isn't
-- stored (it would duplicate every message); `text_hash` detects changes.
CREATE TABLE IF NOT EXISTS windows(
    id INTEGER PRIMARY KEY,
    chat_id INTEGER NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    first_id INTEGER NOT NULL,
    last_id INTEGER NOT NULL,
    text_hash INTEGER NOT NULL,
    embedded INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS windows_chat_end ON windows(chat_id, end_ms);
CREATE INDEX IF NOT EXISTS windows_pending ON windows(id) WHERE embedded = 0;
"#;

impl Store {
    /// Open (creating if needed) the index at `path`.
    pub fn open(path: impl AsRef<Path>, tz: impl Into<crate::tz::Tz>) -> Result<Self, Error> {
        let tz = tz.into();
        let path = path.as_ref().to_path_buf();
        match Self::open_inner(&path, tz) {
            Err(Error::Db(e)) if is_corruption(&e) => {
                // The index is derived from chat.db: set the damaged files
                // aside (one copy kept, for diagnosis) and rebuild.
                for suffix in ["", "-wal", "-shm"] {
                    let from = PathBuf::from(format!("{}{suffix}", path.display()));
                    if from.exists() {
                        std::fs::rename(&from, format!("{}.corrupt{suffix}", path.display()))?;
                    }
                }
                Self::open_inner(&path, tz)
            }
            other => other,
        }
    }

    fn open_inner(path: &Path, tz: crate::tz::Tz) -> Result<Self, Error> {
        let path = path.to_path_buf();
        if let Some(dir) = path.parent() {
            create_private_dir(dir)?;
        }
        let write = open_conn(&path)?;
        let version: Option<i64> = write
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok());
        if version.is_some_and(|v| v != SCHEMA_VERSION) {
            // Derived data only: drop everything and rebuild from chat.db.
            drop(write);
            for suffix in ["", "-wal", "-shm"] {
                let p = PathBuf::from(format!("{}{suffix}", path.display()));
                if p.exists() {
                    std::fs::remove_file(p)?;
                }
            }
            return Self::open_inner(&path, tz);
        }
        write.execute_batch(SCHEMA)?;
        // Added after v2 shipped: migrate in place rather than rebuild (a
        // rebuild re-embeds everything).
        let has_avatar: bool = write
            .prepare("SELECT 1 FROM pragma_table_info('handles') WHERE name='avatar'")?
            .exists([])?;
        if !has_avatar {
            write.execute_batch("ALTER TABLE handles ADD COLUMN avatar TEXT")?;
        }
        write.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', ?1)",
            [SCHEMA_VERSION.to_string()],
        )?;
        let read = (0..READERS)
            .map(|_| open_conn(&path).map(Mutex::new))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Store {
            path,
            write: Mutex::new(write),
            read,
            tz,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn reader(&self) -> MutexGuard<'_, Connection> {
        for c in &self.read {
            if let Ok(g) = c.try_lock() {
                return g;
            }
        }
        self.read[0].lock().unwrap_or_else(|e| e.into_inner())
    }

    fn writer(&self) -> MutexGuard<'_, Connection> {
        self.write.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn tz(&self) -> crate::tz::Tz {
        self.tz
    }

    // ------------------------------------------------------------- meta ---

    pub fn meta(&self, key: &str) -> Result<Option<String>, Error> {
        Ok(self
            .reader()
            .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), Error> {
        self.writer().execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            [key, value],
        )?;
        Ok(())
    }

    // ----------------------------------------------------------- ingest ---

    /// Insert or update handles, then refresh chat titles that use them.
    pub fn upsert_handles(&self, handles: &[IngestHandle]) -> Result<(), Error> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO handles(id, address, name, digits) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET address=excluded.address,
                     name=COALESCE(excluded.name, handles.name), digits=excluded.digits",
            )?;
            for h in handles {
                let digits: String = h.address.chars().filter(char::is_ascii_digit).collect();
                let digits = (digits.len() >= 7).then_some(digits);
                stmt.execute(params![h.id, h.address, h.name, digits])?;
            }
        }
        refresh_titles(&tx, None)?;
        tx.commit()?;
        Ok(())
    }

    /// Where contact photos are stored: `avatars/` next to the index.
    pub fn avatars_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map_or_else(|| PathBuf::from("avatars"), |p| p.join("avatars"))
    }

    /// Set contact photo paths by handle id (None clears).
    pub fn set_handle_avatars(&self, avatars: &HashMap<i64, Option<String>>) -> Result<(), Error> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut stmt =
                tx.prepare_cached("UPDATE handles SET avatar=?2 WHERE id=?1 AND avatar IS NOT ?2")?;
            for (id, path) in avatars {
                stmt.execute(params![id, path])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Set contact names by handle id (None clears). Returns the chats whose
    /// participants' names changed: their titles are refreshed here, and
    /// their windows (which contain names) need rebuilding by the caller.
    pub fn set_handle_names(
        &self,
        names: &HashMap<i64, Option<String>>,
    ) -> Result<Vec<i64>, Error> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let mut chats: Vec<i64> = Vec::new();
        {
            let mut stmt =
                tx.prepare_cached("UPDATE handles SET name=?2 WHERE id=?1 AND name IS NOT ?2")?;
            let mut of =
                tx.prepare_cached("SELECT chat_id FROM chat_handles WHERE handle_id=?1")?;
            for (id, name) in names {
                if stmt.execute(params![id, name])? > 0 {
                    let rows = of.query_map([id], |r| r.get::<_, i64>(0))?;
                    for c in rows {
                        chats.push(c?);
                    }
                }
            }
        }
        chats.sort_unstable();
        chats.dedup();
        if !chats.is_empty() {
            refresh_titles(&tx, Some(&chats))?;
            // Window text contains names: rebuild those chats from the start.
            mark_dirty(&tx, &chats.iter().map(|c| (*c, i64::MIN / 2)).collect())?;
        }
        tx.commit()?;
        Ok(chats)
    }

    pub fn upsert_chats(&self, chats: &[IngestChat]) -> Result<(), Error> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut chat_stmt = tx.prepare_cached(
                "INSERT INTO chats(id, identifier, display_name, service, is_group)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET identifier=excluded.identifier,
                     display_name=excluded.display_name, service=excluded.service,
                     is_group=excluded.is_group",
            )?;
            let mut del = tx.prepare_cached("DELETE FROM chat_handles WHERE chat_id=?1")?;
            let mut ins = tx.prepare_cached(
                "INSERT OR IGNORE INTO chat_handles(chat_id, handle_id) VALUES (?1, ?2)",
            )?;
            for c in chats {
                let is_group = c.participants.len() > 1 || c.identifier.starts_with("chat");
                let name = c.display_name.as_deref().filter(|s| !s.trim().is_empty());
                chat_stmt.execute(params![c.id, c.identifier, name, c.service, is_group])?;
                del.execute([c.id])?;
                for h in &c.participants {
                    ins.execute([c.id, *h])?;
                }
            }
        }
        let ids: Vec<i64> = chats.iter().map(|c| c.id).collect();
        refresh_titles(&tx, Some(&ids))?;
        tx.commit()?;
        Ok(())
    }

    /// Insert or update messages (edits and unsends arrive as updates).
    /// Returns, per chat touched, the earliest message date seen, so the
    /// caller can rebuild that chat's windows from there.
    pub fn ingest_messages(&self, msgs: &[IngestMessage]) -> Result<BTreeMap<i64, i64>, Error> {
        let mut touched: BTreeMap<i64, i64> = BTreeMap::new();
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut upsert = tx.prepare_cached(
                "INSERT INTO messages(source_id, guid, chat_id, handle_id, from_me, date_ms, text, kind,
                     service, reply_to, edited, unsent, has_link, attach_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(source_id) DO UPDATE SET text=excluded.text, chat_id=excluded.chat_id,
                     kind=excluded.kind, edited=excluded.edited, unsent=excluded.unsent,
                     has_link=excluded.has_link, attach_count=excluded.attach_count
                 WHERE messages.text IS NOT excluded.text OR messages.chat_id IS NOT excluded.chat_id
                    OR messages.edited != excluded.edited OR messages.unsent != excluded.unsent
                    OR messages.attach_count != excluded.attach_count
                 RETURNING id",
            )?;
            let mut del_att = tx.prepare_cached("DELETE FROM attachments WHERE message_id=?1")?;
            let mut ins_att = tx.prepare_cached(
                "INSERT OR REPLACE INTO attachments(id, message_id, filename, mime, path, bytes, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            let mut ins_react = tx.prepare_cached(
                "INSERT OR REPLACE INTO reactions(id, target_guid, part, handle_id, from_me, emoji, date_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            // A removal cancels that sender's earlier reaction on the same part.
            let mut del_react = tx.prepare_cached(
                "DELETE FROM reactions WHERE target_guid=?1 AND part=?2 AND from_me=?3
                     AND (handle_id IS ?4) AND date_ms <= ?5",
            )?;

            for m in msgs {
                if let Some(r) = &m.reaction {
                    // Each sender has at most one reaction per part: a new one replaces.
                    del_react.execute(params![
                        r.target_guid,
                        r.part,
                        m.from_me,
                        m.handle_id,
                        m.date_ms
                    ])?;
                    if !r.removed {
                        ins_react.execute(params![
                            m.id,
                            r.target_guid,
                            r.part,
                            m.handle_id,
                            m.from_me,
                            r.emoji,
                            m.date_ms
                        ])?;
                    }
                    continue;
                }
                let text = m.text.as_deref().map(str::trim).filter(|t| !t.is_empty());
                let has_link = text.is_some_and(|t| {
                    t.contains("http://") || t.contains("https://") || t.contains("www.")
                });
                // No row back: already indexed and unchanged (re-reads of
                // recent days are common), so the chat isn't touched.
                let id: Option<i64> = upsert
                    .query_row(
                        params![
                            m.id,
                            m.guid,
                            m.chat_id,
                            m.handle_id,
                            m.from_me,
                            m.date_ms,
                            text,
                            m.kind.code(),
                            m.service,
                            m.reply_to_guid,
                            m.edited,
                            m.unsent,
                            has_link,
                            m.attachments.len() as i64,
                        ],
                        |r| r.get(0),
                    )
                    .optional()?;
                let Some(id) = id else { continue };
                del_att.execute([id])?;
                for a in &m.attachments {
                    ins_att.execute(params![
                        a.id,
                        id,
                        a.filename,
                        a.mime,
                        a.path,
                        a.bytes,
                        a.kind.code()
                    ])?;
                }
                if let Some(chat) = m.chat_id {
                    let e = touched.entry(chat).or_insert(m.date_ms);
                    *e = (*e).min(m.date_ms);
                }
            }
            // Chat list stats for touched chats.
            let mut stats = tx.prepare_cached(
                "UPDATE chats SET
                    last_ms = (SELECT MAX(date_ms) FROM messages WHERE chat_id = ?1),
                    message_count = (SELECT COUNT(*) FROM messages WHERE chat_id = ?1)
                 WHERE id = ?1",
            )?;
            for chat in touched.keys() {
                stats.execute([chat])?;
            }
            mark_dirty(&tx, &touched)?;
        }
        tx.commit()?;
        Ok(touched)
    }

    /// Every chat.db message ROWID in the index (to find deleted rows).
    pub fn source_ids(&self) -> Result<Vec<i64>, Error> {
        let conn = self.reader();
        let mut s = conn.prepare("SELECT source_id FROM messages")?;
        let rows = s.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Remove messages that no longer exist in chat.db (deleted conversations).
    /// `source_ids` are chat.db ROWIDs. Returns, per chat touched, the
    /// earliest deleted message date.
    pub fn delete_messages(&self, source_ids: &[i64]) -> Result<BTreeMap<i64, i64>, Error> {
        let mut touched: BTreeMap<i64, i64> = BTreeMap::new();
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut get =
                tx.prepare_cached("SELECT id, chat_id, date_ms FROM messages WHERE source_id=?1")?;
            let mut del = tx.prepare_cached("DELETE FROM messages WHERE id=?1")?;
            let mut del_att = tx.prepare_cached("DELETE FROM attachments WHERE message_id=?1")?;
            for sid in source_ids {
                let Some((id, chat, date)) = get
                    .query_row([sid], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, Option<i64>>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    })
                    .optional()?
                else {
                    continue;
                };
                if let Some(chat) = chat {
                    let e = touched.entry(chat).or_insert(date);
                    *e = (*e).min(date);
                }
                del.execute([id])?;
                del_att.execute([id])?;
            }
            let mut stats = tx.prepare_cached(
                "UPDATE chats SET
                    last_ms = (SELECT MAX(date_ms) FROM messages WHERE chat_id = ?1),
                    message_count = (SELECT COUNT(*) FROM messages WHERE chat_id = ?1)
                 WHERE id = ?1",
            )?;
            for chat in touched.keys() {
                stats.execute([chat])?;
            }
            mark_dirty(&tx, &touched)?;
        }
        tx.commit()?;
        Ok(touched)
    }

    // ---------------------------------------------------------- windows ---

    /// Rebuild the embedding windows of `chat_id` from `from_ms` on. Windows
    /// that end within one gap of `from_ms` are rebuilt too, since a new
    /// message may extend them. Returns the number of windows written.
    pub fn rebuild_windows(&self, chat_id: i64, from_ms: i64) -> Result<usize, Error> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        let has_vec = vec_table_exists(&tx)?;

        // Earliest start among windows we're about to replace.
        let cut = from_ms - WINDOW_GAP_MS;
        let start: i64 = tx
            .query_row(
                "SELECT MIN(start_ms) FROM windows WHERE chat_id=?1 AND end_ms >= ?2",
                params![chat_id, cut],
                |r| r.get::<_, Option<i64>>(0),
            )?
            .map_or(from_ms, |s| s.min(from_ms));
        // Old windows by (start, text hash): an identical rebuilt window keeps
        // its id and embedding instead of being embedded again.
        let mut old: HashMap<(i64, i64), i64> = {
            let mut s = tx.prepare_cached(
                "SELECT id, start_ms, text_hash FROM windows WHERE chat_id=?1 AND end_ms >= ?2",
            )?;
            let rows = s.query_map(params![chat_id, cut], |r| {
                Ok(((r.get(1)?, r.get(2)?), r.get(0)?))
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let title = chat_title(&tx, chat_id)?;
        let msgs = window_messages(&tx, chat_id, (start, i64::MIN), (i64::MAX, i64::MAX))?;

        let mut windows: Vec<Vec<WindowMsg>> = Vec::new();
        let mut cur: Vec<WindowMsg> = Vec::new();
        let mut cur_chars = 0usize;
        for m in msgs {
            let Some(line) = m.line() else { continue };
            let gap = cur
                .last()
                .is_some_and(|l: &WindowMsg| m.date_ms - l.date_ms > WINDOW_GAP_MS);
            let full =
                cur_chars + line.len() > WINDOW_MAX_CHARS || cur.len() >= WINDOW_MAX_MESSAGES;
            if !cur.is_empty() && (gap || full) {
                windows.push(std::mem::take(&mut cur));
                cur_chars = 0;
            }
            cur_chars += line.len() + 1;
            cur.push(m);
        }
        if !cur.is_empty() {
            windows.push(cur);
        }

        let mut ins = tx.prepare_cached(
            "INSERT INTO windows(chat_id, start_ms, end_ms, first_id, last_id, text_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut keep =
            tx.prepare_cached("UPDATE windows SET end_ms=?2, first_id=?3, last_id=?4 WHERE id=?1")?;
        for w in &windows {
            let (first, last) = (&w[0], &w[w.len() - 1]);
            let hash = text_hash(&window_text(&title, first.date_ms, self.tz, w));
            match old.remove(&(first.date_ms, hash)) {
                Some(id) => keep.execute(params![id, last.date_ms, first.id, last.id])?,
                None => ins.execute(params![
                    chat_id,
                    first.date_ms,
                    last.date_ms,
                    first.id,
                    last.id,
                    hash
                ])?,
            };
        }
        drop(ins);
        drop(keep);
        for id in old.into_values() {
            tx.execute("DELETE FROM windows WHERE id=?1", [id])?;
            if has_vec {
                tx.execute("DELETE FROM vec_windows WHERE rowid=?1", [id])?;
            }
        }
        // Everything from `start` on is rebuilt now.
        tx.execute(
            "DELETE FROM dirty_windows WHERE chat_id=?1 AND from_ms >= ?2",
            params![chat_id, start],
        )?;
        tx.commit()?;
        Ok(windows.len())
    }

    /// Chats whose windows still need rebuilding, and from when (see
    /// `dirty_windows`). Includes work left over from an interrupted run.
    pub fn dirty_chats(&self) -> Result<BTreeMap<i64, i64>, Error> {
        let conn = self.reader();
        let mut s = conn.prepare_cached("SELECT chat_id, from_ms FROM dirty_windows")?;
        let rows = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Rebuild every chat's windows from scratch (after a full ingest or a
    /// contact-name change).
    pub fn rebuild_all_windows(&self) -> Result<usize, Error> {
        let chats: Vec<i64> = {
            let conn = self.reader();
            let mut s = conn.prepare("SELECT id FROM chats")?;
            let rows = s.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut n = 0;
        for chat in chats {
            n += self.rebuild_windows(chat, i64::MIN / 2)?;
        }
        Ok(n)
    }

    // ------------------------------------------------------- embeddings ---

    /// Make sure the vector table matches `model_id`/`dims`. A different model
    /// drops all stored vectors so they get re-embedded.
    pub fn ensure_vectors(&self, model_id: &str, dims: usize) -> Result<(), Error> {
        let current = self.meta("embed_model")?;
        let want = format!("{model_id}:{dims}");
        let mut conn = self.writer();
        if current.as_deref() != Some(want.as_str()) || !vec_table_exists(&conn)? {
            let tx = conn.transaction()?;
            tx.execute_batch("DROP TABLE IF EXISTS vec_windows")?;
            tx.execute_batch(&format!(
                "CREATE VIRTUAL TABLE vec_windows USING vec0(
                    embedding int8[{dims}] distance_metric=cosine,
                    chat_id integer,
                    start_ms integer
                )"
            ))?;
            tx.execute("UPDATE windows SET embedded=0", [])?;
            tx.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES ('embed_model', ?1)",
                [&want],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Mark windows that couldn't be embedded (their text breaks the model)
    /// so they're skipped instead of retried forever.
    pub fn mark_unembeddable(&self, ids: &[i64]) -> Result<(), Error> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut s = tx.prepare_cached("UPDATE windows SET embedded=-1 WHERE id=?1")?;
            for id in ids {
                s.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// sqlite-vec only reuses deleted slots in its newest chunk, so window
    /// churn (every new message rewrites a chat's last window) leaves holes
    /// that KNN still scans and that never shrink. When over ~30% of slots
    /// are dead, rewrite the app's own vector table into a fresh one (the
    /// same vectors, copied, in one transaction). Returns whether it did.
    pub fn compact_vectors_if_needed(&self) -> Result<bool, Error> {
        let Some(model) = self.meta("embed_model")? else {
            return Ok(false);
        };
        let Some(dims) = model
            .rsplit_once(':')
            .and_then(|(_, d)| d.parse::<usize>().ok())
        else {
            return Ok(false);
        };
        let mut conn = self.writer();
        if !vec_table_exists(&conn)? {
            return Ok(false);
        }
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM vec_windows", [], |r| r.get(0))?;
        let chunks: i64 =
            conn.query_row("SELECT COUNT(*) FROM vec_windows_chunks", [], |r| r.get(0))?;
        let slots = (chunks * 1024) as f64;
        if chunks <= 1 || slots <= 1.3 * rows as f64 + 1024.0 {
            return Ok(false);
        }
        // vec0 can't be renamed (its shadow tables keep the old name), so the
        // vectors go through a temp table and the same table is recreated.
        // One transaction: a crash leaves the old table intact. (No ORDER BY
        // on the copy back: a sorter drops vec_int8's int8 subtype.)
        let tx = conn.transaction()?;
        tx.execute_batch(&format!(
            "CREATE TEMP TABLE vec_compact AS
                SELECT rowid AS id, embedding, chat_id, start_ms FROM vec_windows;
            DROP TABLE vec_windows;
            CREATE VIRTUAL TABLE vec_windows USING vec0(
                embedding int8[{dims}] distance_metric=cosine,
                chat_id integer,
                start_ms integer
            );
            INSERT INTO vec_windows(rowid, embedding, chat_id, start_ms)
                SELECT id, vec_int8(embedding), chat_id, start_ms FROM temp.vec_compact;
            DROP TABLE temp.vec_compact;"
        ))?;
        tx.commit()?;
        Ok(true)
    }

    /// Fold the WAL into the database (after a sync or a round of embedding;
    /// journal_size_limit then shrinks the file) and refresh planner stats.
    pub fn checkpoint(&self) -> Result<(), Error> {
        let conn = self.writer();
        conn.execute_batch("PRAGMA optimize;")?;
        conn.query_row("PRAGMA wal_checkpoint(RESTART)", [], |_| Ok(()))?;
        Ok(())
    }

    /// Windows still waiting for an embedding (newest first, so recent
    /// conversations become searchable by meaning first), with their text.
    pub fn pending_windows(&self, limit: usize) -> Result<Vec<(i64, String)>, Error> {
        let conn = self.reader();
        let pending: Vec<(i64, i64, i64, i64, i64, i64)> = conn
            .prepare_cached(
                "SELECT id, chat_id, start_ms, first_id, end_ms, last_id FROM windows
                 WHERE embedded=0 ORDER BY id DESC LIMIT ?1",
            )?
            .query_map([limit as i64], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        let mut out = Vec::with_capacity(pending.len());
        for (id, chat, start, first, end, last) in pending {
            let title = chat_title(&conn, chat)?;
            let msgs: Vec<WindowMsg> = window_messages(&conn, chat, (start, first), (end, last))?
                .into_iter()
                .filter(|m| m.line().is_some())
                .collect();
            out.push((id, window_text(&title, start, self.tz, &msgs)));
        }
        Ok(out)
    }

    /// The (chat, first, last) span of a window, for finding its messages.
    pub(crate) fn window_span_sql() -> &'static str {
        "m.chat_id = w.chat_id AND (m.date_ms, m.id) >= (w.start_ms, w.first_id)
         AND (m.date_ms, m.id) <= (w.end_ms, w.last_id)"
    }

    pub fn pending_window_count(&self) -> Result<i64, Error> {
        Ok(self
            .reader()
            .query_row("SELECT COUNT(*) FROM windows WHERE embedded=0", [], |r| {
                r.get(0)
            })?)
    }

    /// Store embeddings for windows. Windows deleted meanwhile are skipped.
    pub fn store_embeddings(&self, items: &[(i64, Vec<f32>)]) -> Result<(), Error> {
        let mut conn = self.writer();
        let tx = conn.transaction()?;
        {
            let mut meta =
                tx.prepare_cached("SELECT chat_id, start_ms FROM windows WHERE id=?1")?;
            // vec0 rejects INSERT OR REPLACE on an existing rowid.
            let mut del = tx.prepare_cached("DELETE FROM vec_windows WHERE rowid=?1")?;
            let mut ins = tx.prepare_cached(
                "INSERT INTO vec_windows(rowid, embedding, chat_id, start_ms)
                 VALUES (?1, vec_quantize_int8(?2, 'unit'), ?3, ?4)",
            )?;
            let mut mark = tx.prepare_cached("UPDATE windows SET embedded=1 WHERE id=?1")?;
            for (id, v) in items {
                let Some((chat, start)): Option<(i64, i64)> = meta
                    .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?
                else {
                    continue;
                };
                del.execute([id])?;
                ins.execute(params![id, f32_blob(v), chat, start])?;
                mark.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ------------------------------------------------------------ views ---

    pub fn stats(&self) -> Result<Stats, Error> {
        let conn = self.reader();
        let count =
            |sql: &str| -> Result<i64, Error> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
        // Two ORDER BY ... LIMIT 1 lookups use the date index; MIN and MAX in
        // one query would scan it.
        let oldest: Option<i64> = conn
            .query_row(
                "SELECT date_ms FROM messages ORDER BY date_ms LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let newest: Option<i64> = conn
            .query_row(
                "SELECT date_ms FROM messages ORDER BY date_ms DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let mut bytes = 0u64;
        for suffix in ["", "-wal"] {
            if let Ok(m) = std::fs::metadata(format!("{}{suffix}", self.path.display())) {
                bytes += m.len();
            }
        }
        let windows = count("SELECT COUNT(*) FROM windows")?;
        Ok(Stats {
            messages: count("SELECT COUNT(*) FROM messages")?,
            chats: count("SELECT COUNT(*) FROM chats WHERE message_count > 0")?,
            attachments: count("SELECT COUNT(*) FROM attachments")?,
            windows,
            // Pending windows use a partial index; counting embedded ones would scan.
            embedded_windows: windows - count("SELECT COUNT(*) FROM windows WHERE embedded=0")?,
            oldest_ms: oldest,
            newest_ms: newest,
            index_bytes: bytes,
        })
    }

    /// Chats, most recent first.
    pub fn list_chats(&self, limit: usize, offset: usize) -> Result<Vec<ChatSummary>, Error> {
        let conn = self.reader();
        let mut s = conn.prepare_cached(
            "SELECT c.id, c.title, c.is_group, c.last_ms, c.message_count,
                    (SELECT text FROM messages WHERE chat_id=c.id AND kind=0 ORDER BY date_ms DESC LIMIT 1)
             FROM chats c WHERE c.message_count > 0
             ORDER BY c.last_ms DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = s.query_map([limit as i64, offset as i64], |r| {
            Ok(ChatSummary {
                id: r.get(0)?,
                title: r.get(1)?,
                is_group: r.get(2)?,
                participants: Vec::new(),
                last_ms: r.get(3)?,
                message_count: r.get(4)?,
                last_text: r.get(5)?,
            })
        })?;
        let mut out: Vec<ChatSummary> = rows.collect::<Result<_, _>>()?;
        for c in &mut out {
            c.participants = participants(&conn, c.id)?;
        }
        Ok(out)
    }

    pub fn chat(&self, chat_id: i64) -> Result<Option<ChatSummary>, Error> {
        let conn = self.reader();
        let c = conn
            .query_row(
                "SELECT id, title, is_group, last_ms, message_count FROM chats WHERE id=?1",
                [chat_id],
                |r| {
                    Ok(ChatSummary {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        is_group: r.get(2)?,
                        participants: Vec::new(),
                        last_ms: r.get(3)?,
                        message_count: r.get(4)?,
                        last_text: None,
                    })
                },
            )
            .optional()?;
        match c {
            Some(mut c) => {
                c.participants = participants(&conn, c.id)?;
                Ok(Some(c))
            }
            None => Ok(None),
        }
    }

    /// `before` messages before and `after` after `message_id` in its chat,
    /// plus the message itself, oldest first.
    pub fn messages_around(
        &self,
        message_id: i64,
        before: usize,
        after: usize,
    ) -> Result<Vec<MessageView>, Error> {
        let conn = self.reader();
        let anchor: Option<(Option<i64>, i64)> = conn
            .query_row(
                "SELECT chat_id, date_ms FROM messages WHERE id=?1",
                [message_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((Some(chat), date)) = anchor else {
            return Ok(Vec::new());
        };
        let mut older = load_messages(
            &conn,
            "WHERE m.chat_id=?1 AND (m.date_ms < ?2 OR (m.date_ms = ?2 AND m.id < ?3))
             ORDER BY m.date_ms DESC, m.id DESC LIMIT ?4",
            params![chat, date, message_id, before as i64],
        )?;
        older.reverse();
        let newer = load_messages(
            &conn,
            "WHERE m.chat_id=?1 AND (m.date_ms > ?2 OR (m.date_ms = ?2 AND m.id >= ?3))
             ORDER BY m.date_ms, m.id LIMIT ?4",
            params![chat, date, message_id, after as i64 + 1],
        )?;
        older.extend(newer);
        attach_extras(&conn, &mut older)?;
        Ok(older)
    }

    /// Page through a chat: `limit` messages older than (`before_ms`,`before_id`)
    /// or, with `newer`, newer than it. Oldest first.
    pub fn messages_page(
        &self,
        chat_id: i64,
        cursor_ms: i64,
        cursor_id: i64,
        newer: bool,
        limit: usize,
    ) -> Result<Vec<MessageView>, Error> {
        let conn = self.reader();
        let mut out = if newer {
            load_messages(
                &conn,
                "WHERE m.chat_id=?1 AND (m.date_ms > ?2 OR (m.date_ms = ?2 AND m.id > ?3))
                 ORDER BY m.date_ms, m.id LIMIT ?4",
                params![chat_id, cursor_ms, cursor_id, limit as i64],
            )?
        } else {
            let mut v = load_messages(
                &conn,
                "WHERE m.chat_id=?1 AND (m.date_ms < ?2 OR (m.date_ms = ?2 AND m.id < ?3))
                 ORDER BY m.date_ms DESC, m.id DESC LIMIT ?4",
                params![chat_id, cursor_ms, cursor_id, limit as i64],
            )?;
            v.reverse();
            v
        };
        attach_extras(&conn, &mut out)?;
        Ok(out)
    }

    /// On-disk path of an attachment, when Messages has the file.
    pub fn attachment_path(&self, attachment_id: i64) -> Result<Option<String>, Error> {
        Ok(self
            .reader()
            .query_row(
                "SELECT path FROM attachments WHERE id=?1",
                [attachment_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Remember that an attachment's file was moved to `trash_path`.
    pub fn mark_trashed(
        &self,
        attachment_id: i64,
        trash_path: &str,
        now_ms: i64,
    ) -> Result<(), Error> {
        self.writer().execute(
            "INSERT OR REPLACE INTO trashed_attachments(id, trash_path, trashed_ms) VALUES (?1, ?2, ?3)",
            params![attachment_id, trash_path, now_ms],
        )?;
        Ok(())
    }

    /// An attachment's (path, filename, kind).
    pub fn attachment_file(
        &self,
        attachment_id: i64,
    ) -> Result<Option<AttachmentFile>, Error> {
        Ok(self
            .reader()
            .query_row(
                "SELECT path, filename, kind FROM attachments WHERE id=?1",
                [attachment_id],
                |r| Ok((r.get(0)?, r.get(1)?, AttachmentKind::from_code(r.get(2)?))),
            )
            .optional()?)
    }

    /// Latest messages of a chat, oldest first.
    pub fn latest_messages(&self, chat_id: i64, limit: usize) -> Result<Vec<MessageView>, Error> {
        self.messages_page(chat_id, i64::MAX, i64::MAX, false, limit)
    }
}

// ------------------------------------------------------------- helpers ---

fn mark_dirty(tx: &Connection, chats: &BTreeMap<i64, i64>) -> Result<(), Error> {
    let mut s = tx.prepare_cached(
        "INSERT INTO dirty_windows(chat_id, from_ms) VALUES (?1, ?2)
         ON CONFLICT(chat_id) DO UPDATE SET from_ms = MIN(from_ms, excluded.from_ms)",
    )?;
    for (chat, from) in chats {
        s.execute([chat, from])?;
    }
    Ok(())
}

/// Create `dir` readable only by this user (0700): the index holds a copy of
/// messages that macOS otherwise protects behind Full Disk Access.
pub fn create_private_dir(dir: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// A message as it appears in a window's text.
struct WindowMsg {
    id: i64,
    from_me: bool,
    sender: Option<String>,
    date_ms: i64,
    text: Option<String>,
    attach_count: i64,
}

impl WindowMsg {
    /// "Sarah Chen: text", or None when there's nothing to embed.
    fn line(&self) -> Option<String> {
        let who = if self.from_me {
            "Me"
        } else {
            self.sender.as_deref().unwrap_or("Them")
        };
        let body = match (&self.text, self.attach_count) {
            (Some(t), _) => t.chars().take(WINDOW_MAX_CHARS).collect::<String>(),
            (None, n) if n > 0 => "[attachment]".to_string(),
            _ => return None,
        };
        Some(format!("{who}: {body}"))
    }
}

/// Searchable messages of `chat_id` in [(from_ms, from_id), (to_ms, to_id)], in order.
fn window_messages(
    conn: &Connection,
    chat_id: i64,
    from: (i64, i64),
    to: (i64, i64),
) -> Result<Vec<WindowMsg>, Error> {
    let mut s = conn.prepare_cached(
        "SELECT m.id, m.from_me, COALESCE(h.name, h.address), m.date_ms, m.text, m.attach_count
         FROM messages m LEFT JOIN handles h ON h.id = m.handle_id
         WHERE m.chat_id=?1 AND (m.date_ms, m.id) >= (?2, ?3) AND (m.date_ms, m.id) <= (?4, ?5)
           AND m.kind=0 AND m.unsent=0
         ORDER BY m.date_ms, m.id",
    )?;
    let rows = s.query_map(params![chat_id, from.0, from.1, to.0, to.1], |r| {
        Ok(WindowMsg {
            id: r.get(0)?,
            from_me: r.get(1)?,
            sender: r.get(2)?,
            date_ms: r.get(3)?,
            text: r.get(4)?,
            attach_count: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn chat_title(conn: &Connection, chat_id: i64) -> Result<String, Error> {
    Ok(conn
        .query_row("SELECT title FROM chats WHERE id=?1", [chat_id], |r| {
            r.get(0)
        })
        .optional()?
        .unwrap_or_default())
}

/// What gets embedded: "Chat title · 2024-03-02" then one line per message.
fn window_text(title: &str, start_ms: i64, tz: crate::tz::Tz, msgs: &[WindowMsg]) -> String {
    let mut text = format!("{title} · {}", format_date(start_ms, tz));
    for line in msgs.iter().filter_map(WindowMsg::line) {
        text.push('\n');
        text.push_str(&line);
    }
    text
}

fn is_corruption(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::NotADatabase) | Some(rusqlite::ErrorCode::DatabaseCorrupt)
    )
}

/// FNV-1a 64: a fixed, documented hash. (std's DefaultHasher may change
/// between Rust releases, which would make an app update re-embed every
/// window.)
fn text_hash(s: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h as i64
}

pub(crate) fn f32_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub(crate) fn vec_table_exists(conn: &Connection) -> Result<bool, Error> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE name='vec_windows'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Chat title: its group name, else participant names ("Sarah, Mike +3").
fn refresh_titles(conn: &Connection, only: Option<&[i64]>) -> Result<(), Error> {
    let ids: Vec<i64> = match only {
        Some(ids) => ids.to_vec(),
        None => {
            let mut s = conn.prepare("SELECT id FROM chats")?;
            let rows = s.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        }
    };
    let mut get = conn.prepare_cached("SELECT display_name, identifier FROM chats WHERE id=?1")?;
    let mut people = conn.prepare_cached(
        "SELECT COALESCE(h.name, h.address) FROM chat_handles ch JOIN handles h ON h.id = ch.handle_id
         WHERE ch.chat_id=?1 ORDER BY h.name IS NULL, h.name, h.address",
    )?;
    let mut set = conn.prepare_cached("UPDATE chats SET title=?2 WHERE id=?1")?;
    for id in ids {
        let Some((display, identifier)): Option<(Option<String>, String)> = get
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
        else {
            continue;
        };
        let title = match display {
            Some(d) if !d.trim().is_empty() => d,
            _ => {
                let names: Vec<String> = people
                    .query_map([id], |r| r.get::<_, String>(0))?
                    .collect::<Result<_, _>>()?;
                let first: Vec<String> = names
                    .iter()
                    .take(3)
                    .map(|n| {
                        // First names read better in group titles.
                        if names.len() > 1 && !n.contains('@') && !n.starts_with('+') {
                            n.split_whitespace().next().unwrap_or(n).to_string()
                        } else {
                            n.clone()
                        }
                    })
                    .collect();
                match names.len() {
                    0 => identifier,
                    n if n > 3 => format!("{} +{}", first.join(", "), n - 3),
                    _ => first.join(", "),
                }
            }
        };
        set.execute(params![id, title])?;
    }
    Ok(())
}

pub(crate) fn participants(conn: &Connection, chat_id: i64) -> Result<Vec<Person>, Error> {
    let mut s = conn.prepare_cached(
        "SELECT h.id, h.address, h.name, h.avatar FROM chat_handles ch JOIN handles h ON h.id = ch.handle_id
         WHERE ch.chat_id=?1 ORDER BY h.name IS NULL, h.name, h.address",
    )?;
    let rows = s.query_map([chat_id], |r| {
        Ok(Person {
            handle_id: r.get(0)?,
            address: r.get(1)?,
            name: r.get(2)?,
            avatar: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn load_messages(
    conn: &Connection,
    tail: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<MessageView>, Error> {
    let sql = format!(
        "SELECT m.id, m.guid, m.chat_id, m.from_me, COALESCE(h.name, h.address), m.handle_id,
                m.date_ms, m.text, m.kind, m.service, m.reply_to, m.edited, m.unsent, h.avatar
         FROM messages m LEFT JOIN handles h ON h.id = m.handle_id {tail}"
    );
    let mut s = conn.prepare_cached(&sql)?;
    let rows = s.query_map(params, |r| {
        let from_me: bool = r.get(3)?;
        Ok(MessageView {
            id: r.get(0)?,
            guid: r.get(1)?,
            chat_id: r.get(2)?,
            from_me,
            sender: if from_me { None } else { r.get(4)? },
            sender_handle_id: r.get(5)?,
            date_ms: r.get(6)?,
            text: r.get(7)?,
            kind: MessageKind::from_code(r.get(8)?),
            service: r.get(9)?,
            reply_to_guid: r.get(10)?,
            edited: r.get(11)?,
            unsent: r.get(12)?,
            sender_avatar: if from_me { None } else { r.get(13)? },
            attachments: Vec::new(),
            reactions: Vec::new(),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn attach_extras(conn: &Connection, msgs: &mut [MessageView]) -> Result<(), Error> {
    let mut att = conn.prepare_cached(
        "SELECT id, filename, mime, path, bytes, kind FROM attachments WHERE message_id=?1 ORDER BY id",
    )?;
    let mut react = conn.prepare_cached(
        "SELECT r.emoji, r.from_me, COALESCE(h.name, h.address), r.part
         FROM reactions r LEFT JOIN handles h ON h.id = r.handle_id
         WHERE r.target_guid=?1 ORDER BY r.date_ms",
    )?;
    for m in msgs.iter_mut() {
        m.attachments = att
            .query_map([m.id], |r| {
                Ok(AttachmentView {
                    id: r.get(0)?,
                    filename: r.get(1)?,
                    mime: r.get(2)?,
                    path: r.get(3)?,
                    bytes: r.get(4)?,
                    kind: AttachmentKind::from_code(r.get(5)?),
                })
            })?
            .collect::<Result<_, _>>()?;
        m.reactions = react
            .query_map([&m.guid], |r| {
                let from_me: bool = r.get(1)?;
                Ok(ReactionView {
                    emoji: r.get(0)?,
                    from_me,
                    sender: if from_me { None } else { r.get(2)? },
                    part: r.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
    }
    Ok(())
}

/// `YYYY-MM-DD` in local time (offset in seconds).
pub(crate) fn format_date(ms: i64, tz: crate::tz::Tz) -> String {
    let days = tz.local_day(ms);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod hash_tests {
    #[test]
    fn fnv1a_is_stable() {
        // Reference values of FNV-1a 64; changing them re-embeds every window.
        assert_eq!(super::text_hash("") as u64, 0xcbf29ce484222325);
        assert_eq!(super::text_hash("a") as u64, 0xaf63dc4c8601ec8c);
    }
}
