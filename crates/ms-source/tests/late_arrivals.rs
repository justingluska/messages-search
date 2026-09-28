//! A message that arrives late with an old date (phone offline, iCloud
//! backfill) must be picked up by an incremental sync, not only at the next
//! full sync. Uses a scratch copy of the fictional fixture database.

use std::path::PathBuf;

use ms_core::Store;
use ms_source::{sync, Source};

#[test]
fn incremental_sync_catches_backdated_rows() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/chat-small.db");
    assert!(
        fixture.exists(),
        "missing {}: run python3 scripts/make-fixture.py",
        fixture.display()
    );
    let dir = std::env::temp_dir().join(format!("ms-late-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("chat.db");
    std::fs::copy(&fixture, &db).unwrap();

    let store = Store::open(dir.join("index.db"), 0).unwrap();
    let source = Source::open(&db).unwrap();
    sync::sync(&source, &store, true, false, &|_| {}).unwrap();
    let before = store.stats().unwrap().messages;

    // Backdate a new row (higher ROWID) to 2020, long before the cursor.
    {
        let w = rusqlite::Connection::open(&db).unwrap();
        let chat: i64 = w
            .query_row("SELECT chat_id FROM chat_message_join LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let apple_2020_ns: i64 = (1_577_836_800 - 978_307_200) * 1_000_000_000;
        w.execute(
            "INSERT INTO message (guid, text, handle_id, date, is_from_me, service) VALUES ('LATE-1', 'zebracorn backfill', 0, ?1, 1, 'iMessage')",
            [apple_2020_ns],
        )
        .unwrap();
        let id = w.last_insert_rowid();
        w.execute(
            "INSERT INTO chat_message_join (chat_id, message_id, message_date) VALUES (?1, ?2, ?3)",
            [chat, id, apple_2020_ns],
        )
        .unwrap();
    }
    let source = Source::open(&db).unwrap();
    sync::sync(&source, &store, false, false, &|_| {}).unwrap();
    assert_eq!(store.stats().unwrap().messages, before + 1);
    assert_eq!(store.search("zebracorn", 5, None).unwrap().hits.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
