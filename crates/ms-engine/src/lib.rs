//! The app backend, independent of any UI toolkit. It owns:
//! - the index (`ms_core::Store`), read by `search` and the views,
//! - a worker thread that syncs chat.db into the index (full on start,
//!   incremental when the watcher sees Messages write) and embeds windows,
//! - the embedding models (one for background passages, one for queries, so
//!   a search never waits behind an embedding batch).
//!
//! The UI polls [`Engine::status`] and listens for [`Event`]s.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use ms_core::types::{Embedder, IndexPhase, IndexProgress, SearchResults, Stats};
use ms_core::Store;
use ms_source::watch::ChatDbWatcher;
use ms_source::{sync, Source, SourceError};
use serde::Serialize;

/// Which job a model instance does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    /// Background embedding of windows: should use only part of the CPU so
    /// the Mac stays responsive during the first index.
    Passages,
    /// Search queries: latency matters, one short text at a time.
    Queries,
}

/// Loads an embedding model (downloading it on first use). Called once per
/// role, so a search never waits behind a background embedding batch.
/// The progress callback is shareable so a loader can hand it to download code.
pub type ProgressFn = Arc<dyn Fn(IndexProgress) + Send + Sync>;
pub type EmbedderLoader =
    Arc<dyn Fn(ModelRole, ProgressFn) -> Result<Arc<dyn Embedder>, String> + Send + Sync>;

pub struct Config {
    pub db_path: PathBuf,
    pub index_path: PathBuf,
    /// Time zone for dates in queries, insights and window text.
    pub tz: ms_core::Tz,
    /// None disables meaning search.
    pub embedder: Option<EmbedderLoader>,
    /// Use macOS Contacts for names: asks for access (one system prompt)
    /// before the first index, so results show names instead of numbers.
    /// Off for fixture/demo runs.
    pub use_contacts: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    Ok,
    NeedsFullDiskAccess,
    NoMessagesDb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Semantic {
    Ready,
    DownloadingModel,
    Embedding,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactsState {
    Authorized,
    Denied,
    NotDetermined,
    Unsupported,
}

impl From<ms_source::contacts::ContactsAccess> for ContactsState {
    fn from(a: ms_source::contacts::ContactsAccess) -> Self {
        use ms_source::contacts::ContactsAccess as A;
        match a {
            A::Authorized => ContactsState::Authorized,
            A::Denied => ContactsState::Denied,
            A::NotDetermined => ContactsState::NotDetermined,
            A::Unsupported => ContactsState::Unsupported,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub access: Access,
    pub stats: Stats,
    pub progress: Option<IndexProgress>,
    pub semantic: Semantic,
    pub model_id: Option<String>,
    /// Last indexing error (unreadable/corrupt database, ...), if any.
    pub error: Option<String>,
    /// Contacts permission (names and photos).
    pub contacts: ContactsState,
}

#[derive(Debug, Clone)]
pub enum Event {
    Progress(IndexProgress),
    /// New or changed messages are searchable.
    Changed,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Core(#[from] ms_core::Error),
}

enum Job {
    /// Check access; if granted, index and start watching.
    Start,
    Sync {
        full: bool,
    },
    /// The models are loaded: embed what's pending.
    Embed,
}

struct Inner {
    access: Access,
    progress: Option<IndexProgress>,
    semantic: Semantic,
    model_id: Option<String>,
    error: Option<String>,
    /// Contacts permission as last seen, to notice when it's granted.
    contacts: ContactsState,
    last_emit: Instant,
}

pub struct Engine {
    cfg: Config,
    store: Arc<Store>,
    inner: Mutex<Inner>,
    query_embedder: RwLock<Option<Arc<dyn Embedder>>>,
    passage_embedder: RwLock<Option<Arc<dyn Embedder>>>,
    jobs: Mutex<Sender<Job>>,
    /// A sync is waiting: background embedding yields to it.
    sync_waiting: AtomicBool,
    events: Box<dyn Fn(Event) + Send + Sync>,
}

/// Progress events are throttled to this rate (phase changes always go out).
const EMIT_EVERY: Duration = Duration::from_millis(200);

impl Engine {
    /// Open the index and start the worker. `events` is called from
    /// background threads.
    pub fn start(
        cfg: Config,
        events: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Arc<Engine>, EngineError> {
        let store = Arc::new(Store::open(&cfg.index_path, cfg.tz)?);
        let (tx, rx) = mpsc::channel();
        let semantic = if cfg.embedder.is_some() {
            Semantic::DownloadingModel
        } else {
            Semantic::Unavailable
        };
        let engine = Arc::new(Engine {
            cfg,
            store,
            inner: Mutex::new(Inner {
                access: Access::Ok,
                progress: None,
                semantic,
                model_id: None,
                error: None,
                contacts: ms_source::contacts::access().into(),
                last_emit: Instant::now() - EMIT_EVERY,
            }),
            query_embedder: RwLock::new(None),
            passage_embedder: RwLock::new(None),
            jobs: Mutex::new(tx),
            sync_waiting: AtomicBool::new(false),
            events: Box::new(events),
        });
        // Access is checked synchronously so the first status() is accurate.
        engine.lock().access = check_access(&engine.cfg.db_path);
        let worker = engine.clone();
        std::thread::Builder::new()
            .name("indexer".into())
            .spawn(move || worker.run(rx))
            .expect("spawn indexer thread");
        engine.send(Job::Start);
        Ok(engine)
    }

    /// Where the index (and contact photos) live.
    pub fn data_dir(&self) -> Option<&std::path::Path> {
        self.cfg.index_path.parent()
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn status(&self) -> Result<AppStatus, EngineError> {
        let stats = self.store.stats()?;
        // Granted in System Settings while running: re-read with names.
        let contacts: ContactsState = ms_source::contacts::access().into();
        let granted = {
            let mut i = self.lock();
            let was = std::mem::replace(&mut i.contacts, contacts);
            self.cfg.use_contacts
                && was != ContactsState::Authorized
                && contacts == ContactsState::Authorized
        };
        if granted {
            self.send(Job::Sync { full: true });
        }
        let i = self.lock();
        Ok(AppStatus {
            access: i.access,
            stats,
            progress: i.progress.clone(),
            semantic: i.semantic,
            model_id: i.model_id.clone(),
            error: i.error.clone(),
            contacts: i.contacts,
        })
    }

    /// Search with meaning when the query model is loaded.
    pub fn search(&self, query: &str, limit: usize) -> Result<SearchResults, EngineError> {
        let e = self
            .query_embedder
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        Ok(self.store.search(query, limit, e.as_deref())?)
    }

    /// Call after the user grants Full Disk Access.
    pub fn recheck_access(&self) -> Access {
        let access = check_access(&self.cfg.db_path);
        let was = std::mem::replace(&mut self.lock().access, access);
        if was != Access::Ok && access == Access::Ok {
            self.send(Job::Start);
        }
        access
    }

    /// Re-read all of chat.db (also removes messages deleted there).
    pub fn reindex(&self) {
        self.send(Job::Sync { full: true });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn send(&self, job: Job) {
        if matches!(job, Job::Sync { .. }) {
            self.sync_waiting.store(true, Ordering::Relaxed);
        }
        let _ = self
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .send(job);
    }

    fn progress(&self, p: IndexProgress) {
        let emit = {
            let mut i = self.lock();
            let phase_changed = i.progress.as_ref().map(|x| x.phase) != Some(p.phase);
            let due = i.last_emit.elapsed() >= EMIT_EVERY;
            i.progress = if p.phase == IndexPhase::Idle {
                None
            } else {
                Some(p.clone())
            };
            if phase_changed || due || p.phase == IndexPhase::Idle {
                i.last_emit = Instant::now();
                true
            } else {
                false
            }
        };
        if emit {
            (self.events)(Event::Progress(p));
        }
    }

    // ------------------------------------------------------------ worker ---

    fn run(self: Arc<Self>, rx: Receiver<Job>) {
        let mut source: Option<Source> = None;
        let mut watcher: Option<ChatDbWatcher> = None;
        let mut models_requested = false;
        while let Ok(first) = rx.recv() {
            // Coalesce whatever queued up meanwhile.
            let (mut start, mut sync, mut full, mut embed) = (false, false, false, false);
            for job in std::iter::once(first).chain(rx.try_iter()) {
                match job {
                    Job::Start => start = true,
                    Job::Sync { full: f } => {
                        sync = true;
                        full |= f;
                    }
                    Job::Embed => embed = true,
                }
            }
            self.sync_waiting.store(false, Ordering::Relaxed);

            if start && source.is_none() {
                let access = check_access(&self.cfg.db_path);
                self.lock().access = access;
                if access != Access::Ok {
                    continue;
                }
                match Source::open(&self.cfg.db_path) {
                    Ok(s) => source = Some(s),
                    Err(e) => {
                        let mut i = self.lock();
                        match access_of(&e) {
                            Some(a) => i.access = a,
                            None => i.error = Some(e.to_string()),
                        }
                        continue;
                    }
                }
                if self.cfg.use_contacts
                    && ms_source::contacts::access()
                        == ms_source::contacts::ContactsAccess::NotDetermined
                {
                    // Blocks this thread until the user answers the prompt.
                    ms_source::contacts::request_access();
                    self.lock().contacts = ms_source::contacts::access().into();
                }
                // First run (or schema change): read everything; otherwise
                // still reconcile once per launch to catch deletions.
                sync = true;
                full = true;
                let me = Arc::downgrade(&self);
                watcher = ms_source::watch::watch(&self.cfg.db_path, move || {
                    if let Some(engine) = me.upgrade() {
                        engine.send(Job::Sync { full: false });
                    }
                })
                .ok();
            }
            let Some(src) = source.as_ref() else { continue };

            if sync {
                let before = self.store.stats().map(|s| s.messages).unwrap_or(0);
                match sync::sync(src, &self.store, full, self.cfg.use_contacts, &|p| {
                    self.progress(p)
                }) {
                    Ok(report) => {
                        self.lock().error = None;
                        if full {
                            // A full sync rewrites a lot: fold the WAL now.
                            if let Err(e) = self.store.checkpoint() {
                                eprintln!("checkpoint failed: {e}");
                            }
                        }
                        if report.touched_chats > 0
                            || self.store.stats().map(|s| s.messages).unwrap_or(0) != before
                        {
                            (self.events)(Event::Changed);
                        }
                    }
                    Err(SourceError::NoAccess) => {
                        self.lock().access = Access::NeedsFullDiskAccess;
                        source = None;
                        watcher = None;
                        continue;
                    }
                    Err(e) => {
                        eprintln!("sync failed: {e}");
                        self.lock().error = Some(e.to_string());
                    }
                }
                self.progress(IndexProgress::new(IndexPhase::Idle, 0, 0));
            }

            if !models_requested {
                models_requested = true;
                self.load_models();
            }
            if sync || embed {
                self.embed_pending();
            }
        }
        drop(watcher);
    }

    /// Load both model instances on their own thread (first launch downloads).
    fn load_models(self: &Arc<Self>) {
        let Some(loader) = self.cfg.embedder.clone() else {
            return;
        };
        let me = self.clone();
        std::thread::Builder::new()
            .name("model loader".into())
            .spawn(move || {
                me.lock().semantic = Semantic::DownloadingModel;
                let weak = Arc::downgrade(&me);
                let progress: ProgressFn = Arc::new(move |p| {
                    if let Some(e) = weak.upgrade() {
                        e.progress(p);
                    }
                });
                // Offline on first launch? Keep retrying (30 s, 2 min, then
                // every 10 min) instead of losing meaning search until relaunch.
                let backoff = [30u64, 120, 600];
                let mut attempt = 0usize;
                loop {
                    // Only the query model stays loaded; the passage model is
                    // loaded while there's embedding to do (see embed_pending).
                    match loader(ModelRole::Queries, progress.clone()) {
                        Ok(queries) => {
                            {
                                let mut i = me.lock();
                                i.model_id = Some(queries.model_id().to_string());
                                i.error = None;
                            }
                            *me.query_embedder.write().unwrap_or_else(|e| e.into_inner()) =
                                Some(queries);
                            me.progress(IndexProgress::new(IndexPhase::Idle, 0, 0));
                            me.send(Job::Embed);
                            return;
                        }
                        Err(e) => {
                            eprintln!("embedding model unavailable: {e}");
                            {
                                let mut i = me.lock();
                                i.semantic = Semantic::Unavailable;
                                i.error =
                                    Some(format!("Meaning search is unavailable ({e}). Retrying."));
                            }
                            me.progress(IndexProgress::new(IndexPhase::Idle, 0, 0));
                            let wait = backoff[attempt.min(backoff.len() - 1)];
                            attempt += 1;
                            std::thread::sleep(std::time::Duration::from_secs(wait));
                            me.lock().semantic = Semantic::DownloadingModel;
                        }
                    }
                }
            })
            .expect("spawn model loader");
    }

    fn embed_pending(&self) {
        // Meaning search is only ready once the query model has loaded.
        if self
            .query_embedder
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
        {
            return;
        }
        if self.store.pending_window_count().unwrap_or(0) == 0 {
            self.finish_embedding(false);
            return;
        }
        // The passage model (~200 MB) is loaded only while there's work; it
        // loads from the local cache in a fraction of a second.
        let loaded = self
            .passage_embedder
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let e = match loaded {
            Some(e) => e,
            None => {
                let Some(loader) = self.cfg.embedder.clone() else {
                    return;
                };
                match loader(ModelRole::Passages, Arc::new(|_| {})) {
                    Ok(e) => {
                        *self
                            .passage_embedder
                            .write()
                            .unwrap_or_else(|x| x.into_inner()) = Some(e.clone());
                        e
                    }
                    Err(err) => {
                        eprintln!("couldn't load the embedding model: {err}");
                        return;
                    }
                }
            }
        };
        self.lock().semantic = Semantic::Embedding;
        let result = ms_core::embed::embed_pending(
            &self.store,
            e.as_ref(),
            &|p| self.progress(p),
            &self.sync_waiting,
        );
        match result {
            Ok(n) if n > 0 => (self.events)(Event::Changed),
            Ok(_) => {}
            Err(err) => eprintln!("embedding failed: {err}"),
        }
        // Interrupted by a sync: the sync job re-runs this afterwards.
        if self
            .store
            .pending_window_count()
            .map(|n| n == 0)
            .unwrap_or(false)
        {
            self.finish_embedding(true);
        } else {
            self.lock().semantic = Semantic::Embedding;
        }
        self.progress(IndexProgress::new(IndexPhase::Idle, 0, 0));
    }

    /// Everything is embedded: free the passage model and, after a round of
    /// real work, reclaim dead vector slots and fold the WAL.
    fn finish_embedding(&self, did_work: bool) {
        *self
            .passage_embedder
            .write()
            .unwrap_or_else(|e| e.into_inner()) = None;
        self.lock().semantic = Semantic::Ready;
        if !did_work {
            return;
        }
        if let Err(e) = self.store.compact_vectors_if_needed() {
            eprintln!("vector compaction failed: {e}");
        }
        if let Err(e) = self.store.checkpoint() {
            eprintln!("checkpoint failed: {e}");
        }
    }
}

/// The access problem an error means, or None for other failures.
fn access_of(e: &SourceError) -> Option<Access> {
    match e {
        SourceError::NoAccess => Some(Access::NeedsFullDiskAccess),
        SourceError::NotFound(_) => Some(Access::NoMessagesDb),
        SourceError::Other(_) => None,
    }
}

fn check_access(path: &std::path::Path) -> Access {
    match ms_source::check_access(path) {
        Ok(()) => Access::Ok,
        // An unexpected I/O error isn't a permission problem; opening the
        // database will report it properly.
        Err(e) => access_of(&e).unwrap_or(Access::Ok),
    }
}
