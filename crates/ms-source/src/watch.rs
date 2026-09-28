//! Notice when Messages writes to chat.db. File events (FSEvents) give
//! near-instant updates; a slow poll covers events macOS drops (after sleep,
//! or when the WAL file is swapped). Either way, a change is only reported
//! when chat.db or its WAL actually changed size or modification time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::SourceError;

/// Fallback poll when no file events arrive.
const POLL: Duration = Duration::from_secs(5);
/// Messages writes in bursts; wait for quiet before reporting.
const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct ChatDbWatcher {
    _watcher: RecommendedWatcher,
    stop: Arc<AtomicBool>,
    /// Wakes the thread so dropping doesn't wait out the poll interval.
    wake: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for ChatDbWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.wake.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

type Fingerprint = Vec<Option<(u64, SystemTime)>>;

fn fingerprint(files: &[PathBuf]) -> Fingerprint {
    files
        .iter()
        .map(|f| {
            std::fs::metadata(f)
                .ok()
                .and_then(|m| Some((m.len(), m.modified().ok()?)))
        })
        .collect()
}

/// Call `on_change` (on a background thread) whenever chat.db changes.
pub fn watch(
    db_path: &Path,
    on_change: impl Fn() + Send + 'static,
) -> Result<ChatDbWatcher, SourceError> {
    let dir = db_path
        .parent()
        .ok_or_else(|| SourceError::Other("chat.db has no folder".into()))?
        .to_path_buf();
    let files = vec![
        db_path.to_path_buf(),
        PathBuf::from(format!("{}-wal", db_path.display())),
    ];
    let (tx, rx) = mpsc::channel::<()>();
    let wake = tx.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            let relevant = ev.paths.iter().any(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("chat.db"))
            });
            if relevant {
                let _ = tx.send(());
            }
        }
    })
    .map_err(|e| SourceError::Other(format!("can't watch Messages: {e}")))?;
    // The folder, not the files: the WAL is created and replaced over time.
    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| SourceError::Other(format!("can't watch Messages: {e}")))?;

    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("chat.db watcher".into())
        .spawn(move || {
            let mut last = fingerprint(&files);
            while !stop2.load(Ordering::Relaxed) {
                match rx.recv_timeout(POLL) {
                    Ok(()) => {
                        // Swallow the rest of the burst.
                        while !stop2.load(Ordering::Relaxed) && rx.recv_timeout(DEBOUNCE).is_ok() {}
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                let now = fingerprint(&files);
                if now != last {
                    last = now;
                    on_change();
                }
            }
        })
        .map_err(|e| SourceError::Other(e.to_string()))?;

    Ok(ChatDbWatcher {
        _watcher: watcher,
        stop,
        wake,
        thread: Some(thread),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn reports_writes_once_per_burst() {
        let dir = std::env::temp_dir().join(format!("ms-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("chat.db");
        std::fs::write(&db, b"a").unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let w = watch(&db, move || {
            h.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        for i in 0..5 {
            std::fs::write(format!("{}-wal", db.display()), vec![b'x'; i + 1]).unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while hits.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        // Unrelated files don't count.
        std::fs::write(dir.join("other.txt"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let t = std::time::Instant::now();
        drop(w);
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "drop took {:?}",
            t.elapsed()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
