//! Tauri commands: the backend half of the contract in docs/ARCHITECTURE.md.
//! Each maps 1:1 to a wrapper in src/lib/api.ts (snake_case command name,
//! camelCase args). All index work runs on the blocking pool, never on the
//! async runtime threads.

use std::sync::Arc;

use ms_core::types::{
    AttachmentFilter, AttachmentKind, AttachmentPage, ChatSummary, Insights, MessageView,
    SearchResults, StorageSummary,
};
use ms_engine::{Access, AppStatus, Engine};
use serde::Serialize;
use tauri::State;

use crate::error::{CmdError, CmdResult};

type EngineRef<'a> = State<'a, Arc<Engine>>;

/// Run `f` with the engine on the blocking pool.
async fn blocking<T: Send + 'static>(
    engine: &EngineRef<'_>,
    f: impl FnOnce(&Engine) -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    let engine = Arc::clone(engine);
    tauri::async_runtime::spawn_blocking(move || f(&engine))
        .await
        .map_err(|e| CmdError::internal(e.to_string()))?
}

/// While access is missing, every poll re-checks it, so granting Full Disk
/// Access in System Settings starts indexing without a restart.
#[tauri::command]
pub async fn status(engine: EngineRef<'_>) -> CmdResult<AppStatus> {
    blocking(&engine, |e| {
        if e.status()?.access != Access::Ok {
            e.recheck_access();
        }
        Ok(e.status()?)
    })
    .await
}

/// `query` is the raw search box text (operators included).
#[tauri::command]
pub async fn search(
    engine: EngineRef<'_>,
    query: String,
    limit: Option<u32>,
) -> CmdResult<SearchResults> {
    let limit = limit.unwrap_or(100).min(500) as usize;
    blocking(&engine, move |e| Ok(e.search(&query, limit)?)).await
}

/// Messages around a hit, oldest first. `before`/`after` default to 40.
#[tauri::command]
pub async fn messages_around(
    engine: EngineRef<'_>,
    message_id: i64,
    before: Option<u32>,
    after: Option<u32>,
) -> CmdResult<Vec<MessageView>> {
    let (b, a) = (
        before.unwrap_or(40).min(500) as usize,
        after.unwrap_or(40).min(500) as usize,
    );
    blocking(&engine, move |e| {
        Ok(e.store().messages_around(message_id, b, a)?)
    })
    .await
}

/// A page of a chat strictly older (`newer == false`) or newer than the
/// cursor message `(cursor_ms, cursor_id)`, returned oldest first. `limit`
/// defaults to 60.
#[tauri::command]
pub async fn messages_page(
    engine: EngineRef<'_>,
    chat_id: i64,
    cursor_ms: i64,
    cursor_id: i64,
    newer: bool,
    limit: Option<u32>,
) -> CmdResult<Vec<MessageView>> {
    let limit = limit.unwrap_or(60).min(500) as usize;
    blocking(&engine, move |e| {
        Ok(e.store()
            .messages_page(chat_id, cursor_ms, cursor_id, newer, limit)?)
    })
    .await
}

#[tauri::command]
pub async fn get_chat(engine: EngineRef<'_>, chat_id: i64) -> CmdResult<Option<ChatSummary>> {
    blocking(&engine, move |e| Ok(e.store().chat(chat_id)?)).await
}

#[tauri::command]
pub async fn list_chats(
    engine: EngineRef<'_>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> CmdResult<Vec<ChatSummary>> {
    let (limit, offset) = (
        limit.unwrap_or(200).min(500) as usize,
        offset.unwrap_or(0) as usize,
    );
    blocking(&engine, move |e| Ok(e.store().list_chats(limit, offset)?)).await
}

/// Opens Messages.app on that conversation. Messages has no URL for a chat
/// id, so it's addressed by participants (`sms:/open?addresses=...`), which
/// opens the existing 1:1 or group conversation with exactly those people.
#[tauri::command]
pub async fn open_in_messages(engine: EngineRef<'_>, chat_id: i64) -> CmdResult<()> {
    blocking(&engine, move |e| {
        let chat = e
            .store()
            .chat(chat_id)?
            .ok_or_else(|| CmdError::not_found("conversation not found"))?;
        let addresses: Vec<&str> = chat
            .participants
            .iter()
            .map(|p| p.address.as_str())
            .collect();
        if addresses.is_empty() {
            return Err(CmdError::not_found("conversation has no participants"));
        }
        // Addresses come from chat.db (business sender IDs can contain `&`,
        // `?`, spaces), so each is percent-encoded into the URL.
        let encoded: Vec<String> = addresses.iter().map(|a| percent_encode(a)).collect();
        crate::files::open_url(&format!("sms:/open?addresses={}", encoded.join(",")))
    })
    .await
}

/// System Settings → Privacy & Security → Full Disk Access.
#[tauri::command]
pub async fn open_full_disk_access_settings() -> CmdResult<()> {
    crate::files::open_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
    )
}

/// Activity stats for a calendar year, or all time.
#[tauri::command]
pub async fn insights(engine: EngineRef<'_>, year: Option<i32>) -> CmdResult<Insights> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    blocking(&engine, move |e| Ok(e.store().insights(year, now)?)).await
}

#[tauri::command]
pub async fn storage_summary(engine: EngineRef<'_>) -> CmdResult<StorageSummary> {
    blocking(&engine, |e| Ok(e.store().storage_summary()?)).await
}

#[tauri::command]
pub async fn list_attachments(
    engine: EngineRef<'_>,
    filter: AttachmentFilter,
) -> CmdResult<AttachmentPage> {
    blocking(&engine, move |e| Ok(e.store().list_attachments(&filter)?)).await
}

/// An attachment's local file, or a clear error when it's iCloud-only.
fn attachment_on_disk(
    e: &Engine,
    id: i64,
) -> CmdResult<(std::path::PathBuf, Option<String>, AttachmentKind)> {
    let (path, name, kind) = e
        .store()
        .attachment_file(id)?
        .ok_or_else(|| CmdError::not_found("attachment not found"))?;
    let path = path.ok_or_else(|| CmdError::not_found("attachment has no file"))?;
    Ok((crate::files::checked_messages_file(&path)?, name, kind))
}

/// Opens the attachment in its default app.
#[tauri::command]
pub async fn open_attachment(engine: EngineRef<'_>, attachment_id: i64) -> CmdResult<()> {
    blocking(&engine, move |e| {
        let (p, _, _) = attachment_on_disk(e, attachment_id)?;
        // Files that run code when opened (a texted `.command`) are shown in
        // Finder instead.
        if crate::files::reveal_only(&p) {
            return crate::files::reveal(&p);
        }
        crate::files::open_file(&p)
    })
    .await
}

/// Saves a copy to ~/Downloads; returns the new file's path.
#[tauri::command]
pub async fn save_attachment(engine: EngineRef<'_>, attachment_id: i64) -> CmdResult<String> {
    blocking(&engine, move |e| {
        let (p, name, _) = attachment_on_disk(e, attachment_id)?;
        Ok(crate::files::save_to_downloads(&p, name.as_deref())?
            .to_string_lossy()
            .into_owned())
    })
    .await
}

/// Copies the attachment to the clipboard (images as image + file).
#[tauri::command]
pub async fn copy_attachment(engine: EngineRef<'_>, attachment_id: i64) -> CmdResult<()> {
    let (p, _, _) = blocking(&engine, move |e| attachment_on_disk(e, attachment_id)).await?;
    // NSPasteboard is thread-safe.
    crate::files::copy_to_clipboard(&p)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashFailure {
    id: i64,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashResult {
    trashed: usize,
    bytes: i64,
    failed: Vec<TrashFailure>,
}

/// Moves attachment files to the Trash (restorable). Only files inside
/// ~/Library/Messages/Attachments are touched.
#[tauri::command]
pub async fn trash_attachments(engine: EngineRef<'_>, ids: Vec<i64>) -> CmdResult<TrashResult> {
    blocking(&engine, move |e| {
        let mut result = TrashResult {
            trashed: 0,
            bytes: 0,
            failed: Vec::new(),
        };
        for id in ids {
            let outcome = (|| -> CmdResult<i64> {
                let (p, _, _) = attachment_on_disk(e, id)?;
                let checked = crate::files::checked_attachment(&p.to_string_lossy())?;
                let bytes = std::fs::metadata(&checked)
                    .map(|m| m.len() as i64)
                    .unwrap_or(0);
                let went = crate::files::move_to_trash(&checked)?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as i64);
                e.store().mark_trashed(id, &went.to_string_lossy(), now)?;
                Ok(bytes)
            })();
            match outcome {
                Ok(b) => {
                    result.trashed += 1;
                    result.bytes += b;
                }
                Err(err) => result.failed.push(TrashFailure {
                    id,
                    reason: err.message,
                }),
            }
        }
        Ok(result)
    })
    .await
}

/// System Settings → General → Storage, where Messages' own attachment
/// review deletes from iCloud and every device (there's no API for that).
#[tauri::command]
pub async fn open_storage_settings() -> CmdResult<()> {
    crate::files::open_url("x-apple.systempreferences:com.apple.settings.Storage")
}

/// System Settings → Privacy & Security → Contacts.
#[tauri::command]
pub async fn open_contacts_settings() -> CmdResult<()> {
    crate::files::open_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Contacts",
    )
}

/// Shows the index folder (index.db, models, contact photos) in Finder.
#[tauri::command]
pub async fn reveal_index(engine: EngineRef<'_>) -> CmdResult<()> {
    let dir = engine
        .data_dir()
        .ok_or_else(|| CmdError::not_found("no index folder"))?
        .to_path_buf();
    crate::files::open_url(&format!(
        "file://{}",
        percent_encode_path(&dir.to_string_lossy())
    ))
}

#[tauri::command]
pub fn app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Reveals the attachment's file in Finder.
#[tauri::command]
pub async fn reveal_attachment(engine: EngineRef<'_>, attachment_id: i64) -> CmdResult<()> {
    blocking(&engine, move |e| {
        let (p, _, _) = attachment_on_disk(e, attachment_id)?;
        crate::files::reveal(&p)
    })
    .await
}

/// Full rebuild from chat.db (progress arrives as `index-progress`).
#[tauri::command]
pub async fn reindex(engine: EngineRef<'_>) -> CmdResult<()> {
    engine.reindex();
    Ok(())
}

/// Percent-encode everything outside RFC 3986 unreserved characters (plus
/// `+` and `@`, which are safe in `sms:` addresses).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'+' | b'@') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Percent-encode a filesystem path for a `file://` URL (keeps `/`).
fn percent_encode_path(p: &str) -> String {
    p.split('/')
        .map(percent_encode)
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_sms_addresses() {
        assert_eq!(percent_encode("+15555550101"), "+15555550101");
        assert_eq!(percent_encode("a@b.example"), "a@b.example");
        assert_eq!(percent_encode("Bank&body=hi"), "Bank%26body%3Dhi");
        assert_eq!(percent_encode_path("/Users/a b/x"), "/Users/a%20b/x");
    }
}
