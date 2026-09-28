//! Background embedding of pending conversation windows.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::types::{Embedder, IndexPhase, IndexProgress};
use crate::{Error, Store};

/// Windows embedded per model call. Small batches pad less (texts in a batch
/// are padded to the longest) and measured faster per window than 32.
const BATCH: usize = 8;

/// Embed every pending window, reporting progress. Stops early (without
/// error) when `stop` is set. Returns windows embedded.
///
/// Progress is overall (embedded windows of all windows), not per call: a
/// sync interrupts embedding whenever a message arrives, and a per-call
/// counter would restart at 0% each time.
pub fn embed_pending(
    store: &Store,
    embedder: &dyn Embedder,
    progress: &dyn Fn(IndexProgress),
    stop: &AtomicBool,
) -> Result<usize, Error> {
    store.ensure_vectors(embedder.model_id(), embedder.dims())?;
    let stats = store.stats()?;
    let total = stats.windows.max(0) as u64;
    let mut embedded = stats.embedded_windows.max(0) as u64;
    let mut done = 0usize;
    while !stop.load(Ordering::Relaxed) {
        let pending = store.pending_windows(BATCH)?;
        if pending.is_empty() {
            break;
        }
        let texts: Vec<String> = pending.iter().map(|(_, t)| t.clone()).collect();
        let vectors = match embedder.embed_passages(&texts) {
            Ok(v) if v.len() == pending.len() => v,
            // One bad text must not block everything after it forever: embed
            // the batch one by one and skip the windows the model rejects.
            _ => {
                let mut ok = Vec::new();
                let mut bad = Vec::new();
                for (id, text) in &pending {
                    match embedder.embed_passages(std::slice::from_ref(text)) {
                        Ok(mut v) if v.len() == 1 => ok.push((*id, v.remove(0))),
                        _ => bad.push(*id),
                    }
                }
                store.mark_unembeddable(&bad)?;
                embedded += bad.len() as u64;
                store.store_embeddings(&ok)?;
                done += ok.len();
                embedded += ok.len() as u64;
                progress(IndexProgress::new(
                    IndexPhase::Embedding,
                    embedded,
                    total.max(embedded),
                ));
                continue;
            }
        };
        let items: Vec<(i64, Vec<f32>)> = pending.iter().map(|(id, _)| *id).zip(vectors).collect();
        store.store_embeddings(&items)?;
        done += items.len();
        embedded += items.len() as u64;
        progress(IndexProgress::new(
            IndexPhase::Embedding,
            embedded,
            total.max(embedded),
        ));
    }
    Ok(done)
}
