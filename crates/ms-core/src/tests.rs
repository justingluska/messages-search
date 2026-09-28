//! End-to-end tests of ingest → windows → embeddings → search, with a fake
//! bag-of-words embedder (synonym groups stand in for "meaning").

use crate::types::*;
use crate::Store;

const DAY: i64 = 86_400_000;
const T0: i64 = 1_700_000_000_000; // 2023-11-14

/// Hashes words into 64 buckets; words in the same synonym group share a
/// bucket, so "restaurant" ≈ "dinner" ≈ "eat" without sharing any letters.
struct FakeEmbedder;

const SYNONYMS: &[&[&str]] = &[
    &["restaurant", "dinner", "eat", "tacos", "food"],
    &["wedding", "married", "ceremony"],
    &["code", "gate", "password", "pin"],
];

impl FakeEmbedder {
    fn vec(text: &str) -> Vec<f32> {
        let mut v = vec![0f32; 64];
        for w in text
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
        {
            let bucket = SYNONYMS
                .iter()
                .position(|g| g.contains(&w))
                .unwrap_or_else(|| {
                    3 + (w
                        .bytes()
                        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
                        as usize
                        % 61)
                });
            v[bucket] += 1.0;
        }
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        v.iter().map(|x| x / n).collect()
    }
}

impl Embedder for FakeEmbedder {
    fn model_id(&self) -> &str {
        "fake-bow"
    }
    fn dims(&self) -> usize {
        64
    }
    fn embed_passages(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        Ok(texts.iter().map(|t| Self::vec(t)).collect())
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        Ok(Self::vec(text))
    }
    fn max_distance(&self) -> f64 {
        // One shared bucket among ~15 words is a weak but real match here.
        0.9
    }
}

fn msg(
    id: i64,
    chat: i64,
    handle: Option<i64>,
    from_me: bool,
    date: i64,
    text: &str,
) -> IngestMessage {
    IngestMessage {
        id,
        guid: format!("G{id}"),
        chat_id: Some(chat),
        handle_id: handle,
        from_me,
        date_ms: date,
        text: Some(text.to_string()),
        kind: MessageKind::Text,
        service: Some("iMessage".into()),
        reply_to_guid: None,
        edited: false,
        unsent: false,
        attachments: vec![],
        reaction: None,
    }
}

fn seeded() -> (tempdir::Dir, Store) {
    let dir = tempdir::Dir::new();
    let store = Store::open(dir.path().join("index.db"), 0).unwrap();
    store
        .upsert_handles(&[
            IngestHandle {
                id: 1,
                address: "+15555550101".into(),
                name: Some("Sarah Chen".into()),
            },
            IngestHandle {
                id: 2,
                address: "mike@ross.example".into(),
                name: Some("Mike Ross".into()),
            },
            IngestHandle {
                id: 3,
                address: "+15555550199".into(),
                name: None,
            },
        ])
        .unwrap();
    store
        .upsert_chats(&[
            IngestChat {
                id: 10,
                identifier: "+15555550101".into(),
                display_name: None,
                service: None,
                participants: vec![1],
            },
            IngestChat {
                id: 11,
                identifier: "mike@ross.example".into(),
                display_name: None,
                service: None,
                participants: vec![2],
            },
            IngestChat {
                id: 12,
                identifier: "chat123".into(),
                display_name: Some("Lake House".into()),
                service: None,
                participants: vec![1, 2, 3],
            },
        ])
        .unwrap();
    let mut photo = msg(9, 12, Some(3), false, T0 + 9 * DAY, "look at this view");
    photo.attachments.push(IngestAttachment {
        id: 900,
        filename: Some("IMG_1.HEIC".into()),
        mime: Some("image/heic".into()),
        path: Some("/tmp/IMG_1.HEIC".into()),
        bytes: 1234,
        kind: AttachmentKind::Image,
    });
    let msgs = vec![
        msg(1, 10, Some(1), false, T0, "are you close?"),
        msg(2, 10, Some(1), false, T0 + 60_000, "the gate code is 4471#"),
        msg(3, 10, None, true, T0 + 120_000, "perfect thanks"),
        msg(
            4,
            11,
            Some(2),
            false,
            T0 + 2 * DAY,
            "you have to try Veracruz tacos when you're in Austin",
        ),
        msg(
            5,
            11,
            None,
            true,
            T0 + 2 * DAY + 60_000,
            "adding it to the list",
        ),
        msg(
            6,
            11,
            Some(2),
            false,
            T0 + 30 * DAY,
            "save the date! oct 18, ceremony at the Hotel Van Zandt",
        ),
        msg(
            7,
            12,
            None,
            true,
            T0 + 8 * DAY,
            "new gate code for the lake house is 9020",
        ),
        msg(
            8,
            12,
            Some(1),
            false,
            T0 + 8 * DAY + 60_000,
            "check https://lake.example/rules first",
        ),
        photo,
    ];
    let touched = store.ingest_messages(&msgs).unwrap();
    for (chat, from) in touched {
        store.rebuild_windows(chat, from).unwrap();
    }
    (dir, store)
}

fn embed_all(store: &Store, e: &dyn Embedder) {
    store.ensure_vectors(e.model_id(), e.dims()).unwrap();
    loop {
        let pending = store.pending_windows(64).unwrap();
        if pending.is_empty() {
            break;
        }
        let texts: Vec<String> = pending.iter().map(|p| p.1.clone()).collect();
        let vecs = e.embed_passages(&texts).unwrap();
        let items: Vec<(i64, Vec<f32>)> = pending.iter().map(|p| p.0).zip(vecs).collect();
        store.store_embeddings(&items).unwrap();
    }
}

fn ids(r: &SearchResults) -> Vec<i64> {
    r.hits.iter().map(|h| h.message_id).collect()
}

#[test]
fn keyword_search_and_snippets() {
    let (_d, store) = seeded();
    let r = store.search("gate code", 20, None).unwrap();
    assert_eq!(ids(&r).len(), 2);
    assert!(ids(&r).contains(&2) && ids(&r).contains(&7));
    assert!(!r.semantic);
    let hit = r.hits.iter().find(|h| h.message_id == 2).unwrap();
    assert!(hit.snippet.contains("\u{2}gate\u{3}"), "{}", hit.snippet);
    assert_eq!(hit.chat_title, "Sarah Chen");
    assert_eq!(hit.sender.as_deref(), Some("Sarah Chen"));
    // Natural-language question: no message has every word, so any-word
    // matches fill in (the gate messages rank above others).
    let r = store
        .search("what's the code for the gate at sarah's", 20, None)
        .unwrap();
    assert!(
        ids(&r).starts_with(&[2]) || ids(&r).starts_with(&[7]),
        "{:?}",
        ids(&r)
    );
    // Filler words of a question aren't bolded.
    let r = store.search("where is the gate code", 20, None).unwrap();
    let hit = r.hits.iter().find(|h| h.message_id == 2).unwrap();
    assert!(
        !hit.snippet.contains("\u{2}the\u{3}") && !hit.snippet.contains("\u{2}is\u{3}"),
        "{}",
        hit.snippet
    );
    assert!(hit.snippet.contains("\u{2}gate\u{3}"), "{}", hit.snippet);
    // Prefix matching while typing.
    assert_eq!(ids(&store.search("veracr", 20, None).unwrap()), vec![4]);
}

#[test]
fn snippets_window_long_text() {
    let long = format!(
        "{} the gate code is 4471# {}",
        "blah ".repeat(60),
        "more ".repeat(60)
    );
    let s = crate::search::snippet(&long, &["gate".into(), "code".into()], 80);
    assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
    assert!(s.contains("\u{2}gate\u{3} \u{2}code\u{3}"), "{s}");
    // Word-start only: "code" doesn't mark "encoded".
    let s = crate::search::snippet("encoded code", &["code".into()], 80);
    assert_eq!(s, "encoded \u{2}code\u{3}");
}

#[test]
fn avatars_reach_hits_and_transcripts() {
    let (_d, store) = seeded();
    store
        .set_handle_avatars(
            &[(1, Some("/tmp/avatars/1.jpg".to_string()))]
                .into_iter()
                .collect(),
        )
        .unwrap();
    let r = store.search("gate code from:sarah", 20, None).unwrap();
    let hit = &r.hits[0];
    assert_eq!(hit.people.len(), 1);
    assert_eq!(hit.people[0].avatar.as_deref(), Some("/tmp/avatars/1.jpg"));
    // Group hits list every other participant.
    let r = store.search("gate in:lake", 20, None).unwrap();
    assert_eq!(r.hits[0].people.len(), 3);
    let around = store.messages_around(2, 1, 1).unwrap();
    assert_eq!(
        around[1].sender_avatar.as_deref(),
        Some("/tmp/avatars/1.jpg")
    );
    assert_eq!(around[2].sender_avatar, None); // from me
}

#[test]
fn insights_and_storage() {
    let (_d, store) = seeded();
    let all = store.insights(None, T0 + 30 * DAY).unwrap();
    assert_eq!(all.total_messages, 9);
    assert_eq!(all.sent, 3);
    assert_eq!(all.by_hour.iter().sum::<i64>(), 9);
    assert_eq!(all.by_weekday.iter().sum::<i64>(), 9);
    assert_eq!(all.years, vec![2023]);
    assert_eq!(all.busiest_day.as_ref().unwrap().count, 3); // Sarah's burst on T0
    assert_eq!(all.top_people[0].person.name.as_deref(), Some("Sarah Chen"));
    assert_eq!(all.top_groups[0].title, "Lake House");
    assert_eq!(all.current_streak, 1); // Mike's message on T0+30d
    assert_eq!(all.attachments_count, 1);
    assert_eq!(store.insights(Some(2024), T0).unwrap().total_messages, 0);

    let s = store.storage_summary().unwrap();
    assert_eq!((s.total_count, s.total_bytes), (1, 1234));
    assert_eq!(s.by_chat[0].title, "Lake House");
    let page = store
        .list_attachments(&AttachmentFilter {
            kind: Some(AttachmentKind::Image),
            min_bytes: Some(1000),
            chat_id: None,
            sort: AttachmentSort::Size,
            limit: 50,
            offset: 0,
        })
        .unwrap();
    assert_eq!(page.total_count, 1);
    assert_eq!(page.rows[0].filename.as_deref(), Some("IMG_1.HEIC"));
    assert!(!page.rows[0].on_disk);
    assert!(!page.rows[0].in_trash);
    assert_eq!(s.bytes_on_disk, 0);
    // A trashed file that's still in the Trash reads as in_trash.
    let trash = std::env::temp_dir().join(format!("ms-trash-{}", std::process::id()));
    std::fs::write(&trash, b"x").unwrap();
    store
        .mark_trashed(900, &trash.to_string_lossy(), T0)
        .unwrap();
    let f = AttachmentFilter {
        kind: None,
        min_bytes: None,
        chat_id: None,
        sort: AttachmentSort::Size,
        limit: 5,
        offset: 0,
    };
    assert!(store.list_attachments(&f).unwrap().rows[0].in_trash);
    std::fs::remove_file(&trash).unwrap();
    assert!(!store.list_attachments(&f).unwrap().rows[0].in_trash);
    let none = store
        .list_attachments(&AttachmentFilter {
            kind: Some(AttachmentKind::Video),
            min_bytes: None,
            chat_id: None,
            sort: AttachmentSort::Date,
            limit: 50,
            offset: 0,
        })
        .unwrap();
    assert_eq!((none.total_count, none.rows.len()), (0, 0));
}

#[test]
fn filters() {
    let (_d, store) = seeded();
    assert_eq!(
        ids(&store.search("gate from:sarah", 20, None).unwrap()),
        vec![2]
    );
    assert_eq!(
        ids(&store.search("gate from:me", 20, None).unwrap()),
        vec![7]
    );
    assert_eq!(
        ids(&store.search("gate in:lake", 20, None).unwrap()),
        vec![7]
    );
    assert_eq!(ids(&store.search("has:link", 20, None).unwrap()), vec![8]);
    assert_eq!(ids(&store.search("has:photo", 20, None).unwrap()), vec![9]);
    // Group chat title from participants' first names + unknown number.
    assert_eq!(store.chat(12).unwrap().unwrap().title, "Lake House");
    // in: by participant, and phone digits.
    assert_eq!(
        ids(&store.search("tacos with:mike", 20, None).unwrap()),
        vec![4]
    );
    assert_eq!(
        ids(&store.search("view from:5550199", 20, None).unwrap()),
        vec![9]
    );
    // Nobody called Zed: no results rather than ignoring the filter.
    assert!(store
        .search("gate from:zed", 20, None)
        .unwrap()
        .hits
        .is_empty());
    // Dates (T0 = 2023-11-14).
    let during = store.search("from:mike during:2023-12", 20, None).unwrap();
    assert_eq!(ids(&during), vec![6]);
    let r = store.search("-lake gate", 20, None).unwrap();
    assert_eq!(ids(&r), vec![2]);
}

#[test]
fn meaning_search_finds_synonyms() {
    let (_d, store) = seeded();
    embed_all(&store, &FakeEmbedder);
    let s = store.stats().unwrap();
    assert!(s.windows > 0 && s.embedded_windows == s.windows);

    // "restaurant" appears nowhere; the tacos conversation is found by meaning.
    let r = store.search("restaurant", 20, Some(&FakeEmbedder)).unwrap();
    assert!(r.semantic);
    assert_eq!(
        r.hits.first().map(|h| h.message_id),
        Some(4),
        "{:?}",
        r.hits
    );
    assert_eq!(r.hits[0].matched_by, MatchedBy::Meaning);

    // Keyword + meaning agree → Both.
    let r = store.search("gate code", 20, Some(&FakeEmbedder)).unwrap();
    assert!(r.hits.iter().any(|h| h.matched_by == MatchedBy::Both));

    // Filters apply to meaning hits too.
    let r = store
        .search("wedding from:sarah", 20, Some(&FakeEmbedder))
        .unwrap();
    assert!(r
        .hits
        .iter()
        .all(|h| h.sender.as_deref() == Some("Sarah Chen")));
}

#[test]
fn edits_unsends_reactions_and_context() {
    let (_d, store) = seeded();
    // Edit message 2's text and add a reaction from me, then remove Sarah's.
    let mut edited = msg(2, 10, Some(1), false, T0 + 60_000, "the gate code is 5582#");
    edited.edited = true;
    let react = |id: i64, from_me: bool, handle: Option<i64>, removed: bool| IngestMessage {
        reaction: Some(IngestReaction {
            target_guid: "G2".into(),
            part: 0,
            emoji: "❤️".into(),
            removed,
        }),
        ..msg(id, 10, handle, from_me, T0 + 70_000 + id, "")
    };
    let touched = store
        .ingest_messages(&[
            edited,
            react(20, true, None, false),
            react(21, false, Some(1), false),
            react(22, false, Some(1), true),
        ])
        .unwrap();
    for (c, f) in touched {
        store.rebuild_windows(c, f).unwrap();
    }
    assert!(store.search("4471", 20, None).unwrap().hits.is_empty());
    assert_eq!(ids(&store.search("5582", 20, None).unwrap()), vec![2]);

    let around = store.messages_around(2, 10, 10).unwrap();
    assert_eq!(
        around.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    let m2 = &around[1];
    assert!(m2.edited);
    assert_eq!(m2.reactions.len(), 1, "{:?}", m2.reactions);
    assert!(m2.reactions[0].from_me);

    // Paging.
    let page = store.messages_page(10, T0 + 120_000, 3, false, 1).unwrap();
    assert_eq!(page.iter().map(|m| m.id).collect::<Vec<_>>(), vec![2]);
    let latest = store.latest_messages(12, 50).unwrap();
    assert_eq!(latest.last().unwrap().attachments.len(), 1);

    // Chat list ordering: most recent first.
    let chats = store.list_chats(10, 0).unwrap();
    assert_eq!(chats[0].id, 11);
}

#[test]
fn incremental_windows_extend_and_split() {
    let (_d, store) = seeded();
    embed_all(&store, &FakeEmbedder);
    let before = store.stats().unwrap().windows;
    // A reply 5 minutes later joins Sarah's window; one a day later starts a new one.
    let t = store
        .ingest_messages(&[msg(30, 10, None, true, T0 + 5 * 60_000, "omw")])
        .unwrap();
    for (c, f) in t {
        store.rebuild_windows(c, f).unwrap();
    }
    assert_eq!(store.stats().unwrap().windows, before);
    assert_eq!(store.pending_window_count().unwrap(), 1);
    let t = store
        .ingest_messages(&[msg(31, 10, None, true, T0 + DAY, "dinner friday?")])
        .unwrap();
    for (c, f) in t {
        store.rebuild_windows(c, f).unwrap();
    }
    assert_eq!(store.stats().unwrap().windows, before + 1);
}

#[test]
fn unchanged_reingest_is_a_noop() {
    let (_d, store) = seeded();
    embed_all(&store, &FakeEmbedder);
    // Same rows again (what a periodic re-read of recent days does).
    let again = vec![msg(1, 10, Some(1), false, T0, "are you close?")];
    assert!(store.ingest_messages(&again).unwrap().is_empty());
    // Rebuilding a chat whose text didn't change keeps its embeddings.
    store.rebuild_windows(10, T0).unwrap();
    assert_eq!(store.pending_window_count().unwrap(), 0);
}

#[test]
fn interrupted_window_rebuild_is_resumed() {
    let dir = tempdir::Dir::new();
    let path = dir.path().join("index.db");
    {
        let store = Store::open(&path, 0).unwrap();
        store
            .upsert_chats(&[IngestChat {
                id: 10,
                identifier: "x".into(),
                display_name: None,
                service: None,
                participants: vec![],
            }])
            .unwrap();
        store
            .ingest_messages(&[msg(1, 10, None, true, T0, "hello there")])
            .unwrap();
        // "Crash" before windows are rebuilt.
    }
    let store = Store::open(&path, 0).unwrap();
    // Re-ingesting the same rows changes nothing...
    assert!(store
        .ingest_messages(&[msg(1, 10, None, true, T0, "hello there")])
        .unwrap()
        .is_empty());
    // ...but the pending rebuild survived the restart.
    let dirty = store.dirty_chats().unwrap();
    assert_eq!(dirty.keys().copied().collect::<Vec<_>>(), vec![10]);
    for (c, f) in dirty {
        store.rebuild_windows(c, f).unwrap();
    }
    assert!(store.dirty_chats().unwrap().is_empty());
    assert_eq!(store.stats().unwrap().windows, 1);
}

#[test]
fn vector_churn_is_compacted() {
    let (_d, store) = seeded();
    // 2,200 one-message windows (an hour apart) fill a few 1,024-slot chunks.
    let hour = 3_600_000;
    let base = T0 + 60 * DAY;
    let make = |i: i64, text: &str| msg(5000 + i, 10, None, true, base + i * hour, text);
    let first: Vec<IngestMessage> = (0..2200).map(|i| make(i, &format!("note {i}"))).collect();
    for (c, f) in store.ingest_messages(&first).unwrap() {
        store.rebuild_windows(c, f).unwrap();
    }
    embed_all(&store, &FakeEmbedder);
    // Delete 1,500 of them (a deleted conversation): their slots are freed
    // but never refilled, so most of the vector table is dead space.
    let gone: Vec<i64> = (0..1500).map(|i| 5000 + i).collect();
    for (c, f) in store.delete_messages(&gone).unwrap() {
        store.rebuild_windows(c, f).unwrap();
    }
    let before = store.search("gate code", 20, Some(&FakeEmbedder)).unwrap();
    assert!(
        store.compact_vectors_if_needed().unwrap(),
        "expected dead slots to trigger compaction"
    );
    assert!(
        !store.compact_vectors_if_needed().unwrap(),
        "compaction should leave nothing to do"
    );
    let after = store.search("gate code", 20, Some(&FakeEmbedder)).unwrap();
    assert_eq!(
        before.hits.iter().map(|h| h.message_id).collect::<Vec<_>>(),
        after.hits.iter().map(|h| h.message_id).collect::<Vec<_>>()
    );
    let stats = store.stats().unwrap();
    assert_eq!(stats.embedded_windows, stats.windows);
}

#[test]
fn corrupt_index_is_set_aside_and_rebuilt() {
    let dir = tempdir::Dir::new();
    let path = dir.path().join("index.db");
    std::fs::write(&path, vec![0x42u8; 8192]).unwrap();
    let store = Store::open(&path, 0).unwrap();
    assert_eq!(store.stats().unwrap().messages, 0);
    assert!(dir.path().join("index.db.corrupt").exists());
}

#[test]
fn model_change_resets_vectors() {
    let (_d, store) = seeded();
    embed_all(&store, &FakeEmbedder);
    store.ensure_vectors("other-model", 64).unwrap();
    assert_eq!(
        store.pending_window_count().unwrap(),
        store.stats().unwrap().windows
    );
}

/// Minimal temp dir (no extra dependency).
mod tempdir {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    pub struct Dir(PathBuf);

    impl Dir {
        pub fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "ms-core-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
