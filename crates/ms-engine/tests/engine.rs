//! End to end: fixture chat.db → Engine → search, with a small fake embedder.
//! Needs `fixtures/chat-small.db` (python3 scripts/make-fixture.py).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ms_core::types::{Embedder, MatchedBy};
use ms_engine::{Access, Config, Engine, Event, ModelRole, Semantic};

struct Fake;

impl Embedder for Fake {
    fn model_id(&self) -> &str {
        "fake"
    }
    fn dims(&self) -> usize {
        32
    }
    fn embed_passages(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        Ok(texts.iter().map(|t| vector(t)).collect())
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        Ok(vector(text))
    }
    fn max_distance(&self) -> f64 {
        0.95
    }
}

fn vector(t: &str) -> Vec<f32> {
    let mut v = [0f32; 32];
    for w in t
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3)
    {
        v[w.bytes()
            .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize))
            % 32] += 1.0;
    }
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    v.iter().map(|x| x / n).collect()
}

fn fixture() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/chat-small.db");
    assert!(
        p.exists(),
        "missing {}: run python3 scripts/make-fixture.py",
        p.display()
    );
    p
}

fn wait_for(what: &str, timeout: Duration, mut ok: impl FnMut() -> bool) {
    let t = Instant::now();
    while !ok() {
        assert!(t.elapsed() < timeout, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn indexes_embeds_and_searches() {
    let dir = std::env::temp_dir().join(format!("ms-engine-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let changed = Arc::new(AtomicUsize::new(0));
    let c = changed.clone();
    let loader: ms_engine::EmbedderLoader =
        Arc::new(|_role: ModelRole, _p: ms_engine::ProgressFn| {
            Ok(Arc::new(Fake) as Arc<dyn Embedder>)
        });
    let engine = Engine::start(
        Config {
            db_path: fixture(),
            index_path: dir.join("index.db"),
            tz: ms_core::Tz::Fixed(0),
            embedder: Some(loader),
            use_contacts: false,
        },
        move |e| {
            if matches!(e, Event::Changed) {
                c.fetch_add(1, Ordering::SeqCst);
            }
        },
    )
    .unwrap();

    assert_eq!(engine.status().unwrap().access, Access::Ok);
    wait_for("semantic ready", Duration::from_secs(30), || {
        engine
            .status()
            .map(|s| s.semantic == Semantic::Ready && s.stats.embedded_windows > 0)
            .unwrap_or(false)
    });
    let status = engine.status().unwrap();
    assert!(status.stats.messages > 400, "{status:?}");
    assert_eq!(status.stats.windows, status.stats.embedded_windows);
    assert!(status.progress.is_none());
    assert!(status.error.is_none());
    assert!(changed.load(Ordering::SeqCst) >= 1);

    let r = engine.search("gate code", 20).unwrap();
    assert!(r.semantic);
    assert!(!r.hits.is_empty());
    assert!(r
        .hits
        .iter()
        .any(|h| h.matched_by == MatchedBy::Both || h.matched_by == MatchedBy::Keyword));
    // Context for the first hit renders a conversation around it.
    let around = engine
        .store()
        .messages_around(r.hits[0].message_id, 5, 5)
        .unwrap();
    assert!(around.iter().any(|m| m.id == r.hits[0].message_id));

    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_database_reports_no_messages_db() {
    let dir = std::env::temp_dir().join(format!("ms-engine-missing-{}", std::process::id()));
    let engine = Engine::start(
        Config {
            db_path: dir.join("nope/chat.db"),
            index_path: dir.join("index.db"),
            tz: ms_core::Tz::Fixed(0),
            embedder: None,
            use_contacts: false,
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(engine.status().unwrap().access, Access::NoMessagesDb);
    assert_eq!(engine.status().unwrap().semantic, Semantic::Unavailable);
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}
