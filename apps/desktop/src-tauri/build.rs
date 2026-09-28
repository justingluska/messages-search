// Every app command must be listed here: tauri-build generates an
// `allow-<command>` permission for each, and capabilities/default.json grants
// exactly those. A command missing from either list is unreachable from JS.
const COMMANDS: &[&str] = &[
    "status",
    "search",
    "messages_around",
    "messages_page",
    "get_chat",
    "list_chats",
    "open_in_messages",
    "open_full_disk_access_settings",
    "reveal_attachment",
    "reindex",
    "open_contacts_settings",
    "reveal_index",
    "app_version",
    "insights",
    "storage_summary",
    "list_attachments",
    "open_attachment",
    "save_attachment",
    "copy_attachment",
    "trash_attachments",
    "open_storage_settings",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
