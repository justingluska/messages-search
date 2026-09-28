//! chat.db → index synchronization.

use std::collections::{BTreeMap, HashSet};

use ms_core::types::{IndexPhase, IndexProgress};
use ms_core::Store;

use crate::{contacts, Source, SourceError};

/// Messages converted per index transaction.
const BATCH: usize = 2000;

#[derive(Debug, Default, Clone)]
pub struct SyncReport {
    /// Rows read from chat.db.
    pub read: usize,
    /// Chats whose messages changed.
    pub touched_chats: usize,
    /// Messages removed because they're gone from chat.db (full sync only).
    pub deleted: usize,
    pub windows_written: usize,
}

fn core(e: ms_core::Error) -> SourceError {
    SourceError::Other(e.to_string())
}

/// Sync chat.db into the index. `full` re-reads everything and removes
/// deleted messages; otherwise only recent days are re-read (cheap: unchanged
/// rows are no-ops). Returns what changed.
///
/// `use_contacts`: resolve names through macOS Contacts (off for fixture
/// runs, whose names come from the fixture).
pub fn sync(
    source: &Source,
    store: &Store,
    full: bool,
    use_contacts: bool,
    progress: &dyn Fn(IndexProgress),
) -> Result<SyncReport, SourceError> {
    progress(IndexProgress::new(IndexPhase::Reading, 0, 0));
    let handles = source.handles()?;
    store.upsert_handles(&handles).map_err(core)?;
    store.upsert_chats(&source.chats()?).map_err(core)?;

    // Names go in before windows are built (window text contains names), so
    // the first index doesn't embed everything twice. Chats whose names
    // changed since last time get their windows rebuilt from the start.
    let mut touched: BTreeMap<i64, i64> = BTreeMap::new();
    if full && use_contacts {
        for chat in apply_contacts(&handles, store)? {
            touched.insert(chat, i64::MIN / 2);
        }
    }

    let since = if full {
        None
    } else {
        let last_date = store
            .meta("last_date_ms")
            .map_err(core)?
            .and_then(|v| v.parse::<i64>().ok());
        let last_rowid = store
            .meta("source_max_rowid")
            .map_err(core)?
            .and_then(|v| v.parse::<i64>().ok());
        // Re-read from the older of: the newest date we've seen, and the
        // oldest date among rows added since last time.
        let late = match last_rowid {
            Some(r) => source.min_date_after_rowid(r)?,
            None => None,
        };
        match (last_date, late) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    };
    let total = if since.is_none() {
        source.message_count()?.max(0) as u64
    } else {
        0
    };
    let mut seen: HashSet<i64> = HashSet::new();
    let mut max_date = since.unwrap_or(0);
    let mut converted = 0u64;
    let read = source.messages(since, BATCH, |batch| {
        converted += batch.len() as u64;
        for m in &batch {
            if m.reaction.is_none() {
                seen.insert(m.id);
            }
            max_date = max_date.max(m.date_ms);
        }
        for (chat, from) in store.ingest_messages(&batch).map_err(core)? {
            let e = touched.entry(chat).or_insert(from);
            *e = (*e).min(from);
        }
        progress(IndexProgress::new(
            IndexPhase::Indexing,
            converted,
            total.max(converted),
        ));
        Ok(())
    })?;

    let mut deleted = 0;
    if full {
        let gone: Vec<i64> = store
            .source_ids()
            .map_err(core)?
            .into_iter()
            .filter(|id| !seen.contains(id))
            .collect();
        deleted = gone.len();
        for (chat, from) in store.delete_messages(&gone).map_err(core)? {
            let e = touched.entry(chat).or_insert(from);
            *e = (*e).min(from);
        }
    }

    // Rebuild from the persisted dirty list, not just this run's changes: it
    // also holds work an interrupted run (crash, quit) left behind.
    let dirty = store.dirty_chats().map_err(core)?;
    let mut windows_written = 0;
    let n = dirty.len() as u64;
    for (i, (chat, from)) in dirty.iter().enumerate() {
        windows_written += store.rebuild_windows(*chat, *from).map_err(core)?;
        if i % 20 == 0 {
            progress(IndexProgress::new(IndexPhase::Windows, i as u64, n));
        }
    }
    // A message with a future timestamp (a device with a wrong clock) must
    // not push the cursor past everything that arrives afterwards.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(i64::MAX, |d| d.as_millis() as i64);
    let max_date = max_date.min(now_ms + 86_400_000);
    store
        .set_meta("last_date_ms", &max_date.to_string())
        .map_err(core)?;
    store
        .set_meta("source_max_rowid", &source.max_rowid()?.to_string())
        .map_err(core)?;
    progress(IndexProgress::new(IndexPhase::Idle, 0, 0));
    Ok(SyncReport {
        read,
        touched_chats: touched.len(),
        deleted,
        windows_written,
    })
}

/// Resolve handle names through Contacts (when authorized) and store them.
/// Returns chats whose participants' names changed.
fn apply_contacts(
    handles: &[ms_core::types::IngestHandle],
    store: &Store,
) -> Result<Vec<i64>, SourceError> {
    if contacts::access() != contacts::ContactsAccess::Authorized {
        return Ok(Vec::new());
    }
    let all = contacts::load_contacts().map_err(SourceError::Other)?;
    // Only contacts that were found. With "limited" Contacts access (macOS
    // 26) most people are simply not visible, and that mustn't erase names.
    let found = contacts::resolve_contacts(handles, &all);
    let names: std::collections::HashMap<i64, Option<String>> = found
        .iter()
        .map(|(id, c)| (*id, Some(c.name.clone())))
        .collect();

    // Photos are files next to the index, so the UI can load them directly.
    let dir = store.avatars_dir();
    ms_core::store::create_private_dir(&dir).map_err(core)?;
    let mut avatars: std::collections::HashMap<i64, Option<String>> =
        std::collections::HashMap::new();
    for (id, c) in &found {
        let Some(bytes) = &c.thumbnail else { continue };
        let path = dir.join(format!("{id}.jpg"));
        // Rewrite only when the photo changed.
        if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
            std::fs::write(&path, bytes.as_slice())
                .map_err(|e| SourceError::Other(e.to_string()))?;
        }
        avatars.insert(*id, Some(path.to_string_lossy().into_owned()));
    }
    store.set_handle_avatars(&avatars).map_err(core)?;
    store.set_handle_names(&names).map_err(core)
}
