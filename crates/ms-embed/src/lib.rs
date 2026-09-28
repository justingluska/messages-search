//! Local text embeddings for meaning search: implements
//! [`ms_core::types::Embedder`] on top of fastembed (ONNX Runtime).
//!
//! ## Model files
//! Downloaded once from Hugging Face into the `cache_dir` the caller passes
//! (the app's Application Support dir), in the standard hf-hub cache layout
//! (`models--<org>--<repo>/{blobs,snapshots,refs}`). Each model is pinned to an
//! exact repo commit, so a given [`Embedder::model_id`] always means the same
//! weights and stored vectors stay valid. We fetch the files ourselves (rather
//! than via `TextEmbedding::try_new`) so that:
//! - the cache location is exactly `cache_dir` (fastembed lets `HF_HOME`
//!   override it and otherwise defaults to `./.fastembed_cache`),
//! - no Hugging Face token from the user's machine is ever sent,
//! - the UI gets real byte-level download progress,
//! - an already-downloaded model loads with no network access at all.
//!
//! ## Runtime linking (what the app bundle needs)
//! With `ort/download-binaries` (enabled through fastembed's
//! `ort-download-binaries-native-tls` feature) the build downloads pyke's
//! prebuilt ONNX Runtime 1.28 for `aarch64-apple-darwin` (cached in
//! `~/Library/Caches/ort.pyke.io`) as a **static** `libonnxruntime.a` and links
//! it into the final binary (adds ~20 MB). Nothing extra ships in the .app:
//! `otool -L` on a release binary lists only system libraries (libc++,
//! libSystem, libiconv, libobjc, and the Foundation, CoreFoundation, CoreML and
//! Security frameworks); there is no `libonnxruntime.dylib`. An x86_64 or
//! universal build would need pyke's x86_64 prebuilt plus a `lipo` step; not
//! handled here. CI/offline builds can point `ORT_LIB_LOCATION` at a local copy.
//!
//! ## Execution provider
//! CPU only, on purpose. Measured on an M5 Pro, batches of 64 windows: the
//! CoreML EP gave no speedup for the int8 model (its QDQ ops stay on CPU) and
//! was 6-8x *slower* for the fp32 model (dynamic input shapes force CoreML to
//! recompile per batch shape). ORT's CPU path already uses Apple's matrix units.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fastembed::{
    EmbeddingModel, InitOptionsUserDefined, TextEmbedding, TokenizerFiles,
    UserDefinedEmbeddingModel,
};
use hf_hub::api::sync::{ApiBuilder, ApiRepo};
use hf_hub::api::Progress;
use hf_hub::{Cache, Repo, RepoType};
use ms_core::types::Embedder;

/// Which embedding model to run. Both produce 384-dim vectors, but vectors
/// from different models are not comparable (different [`Embedder::model_id`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelChoice {
    /// BAAI bge-small-en-v1.5, statically int8-quantized ONNX export by Qdrant.
    /// English. ~67 MB download.
    #[default]
    BgeSmallEnV15Q,
    /// intfloat multilingual-e5-small, fp32 ONNX. ~100 languages, but a
    /// ~470 MB download (the 250k-token multilingual vocabulary dominates).
    MultilingualE5Small,
}

impl ModelChoice {
    /// Stable id stored next to vectors; changing it invalidates them.
    pub fn model_id(self) -> &'static str {
        match self {
            ModelChoice::BgeSmallEnV15Q => "bge-small-en-v1.5-q",
            ModelChoice::MultilingualE5Small => "multilingual-e5-small",
        }
    }

    pub fn dims(self) -> usize {
        384
    }

    /// Approximate total download size, for "Downloading model (67 MB)…"
    /// before the real sizes are known. Exact sizes come with [`DownloadEvent`].
    pub fn approx_download_bytes(self) -> u64 {
        match self {
            ModelChoice::BgeSmallEnV15Q => 67_200_000,
            ModelChoice::MultilingualE5Small => 487_400_000,
        }
    }

    fn fastembed_model(self) -> EmbeddingModel {
        match self {
            ModelChoice::BgeSmallEnV15Q => EmbeddingModel::BGESmallENV15Q,
            ModelChoice::MultilingualE5Small => EmbeddingModel::MultilingualE5Small,
        }
    }

    /// Hugging Face commit the files are pinned to.
    fn revision(self) -> &'static str {
        match self {
            ModelChoice::BgeSmallEnV15Q => "aa8f8b060edb00e03bfdd08813a2949946c8ba55",
            ModelChoice::MultilingualE5Small => "614241f622f53c4eeff9890bdc4f31cfecc418b3",
        }
    }

    /// SHA-256 of each file in [`file_names`] order (ONNX, tokenizer.json,
    /// config.json, special_tokens_map.json, tokenizer_config.json) at
    /// [`revision`]. The ONNX hashes match Hugging Face's LFS object ids.
    fn sha256s(self) -> [&'static str; 5] {
        match self {
            ModelChoice::BgeSmallEnV15Q => [
                "51f1bd0addd6e859e42c2c8021a5e5461385bb676a649f4b269aa445449f2431",
                "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
                "13582bcf2effc85b7bf3d3f5532e686bc1c9ce86bb009d10f0ec33cbe92299dd",
                "5d5b662e421ea9fac075174bb0688ee0d9431699900b90662acd44b2a350503a",
                "0b29c7bfc889e53b36d9dd3e686dd4300f6525110eaa98c76a5dafceb2029f53",
            ],
            ModelChoice::MultilingualE5Small => [
                "ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665",
                "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39",
                "69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959",
                "d05497f1da52c5e09554c0cd874037a083e1dc1b9cfd48034d1c717f1afc07a7",
                "a1d6bc8734a6f635dc158508bef000f8e2e5a759c7d92f984b2c86e5ff53425b",
            ],
        }
    }

    /// Prefix for search queries. bge-v1.5 uses an instruction for short
    /// query -> passage retrieval; e5 was trained with "query: "/"passage: ".
    fn query_prefix(self) -> &'static str {
        match self {
            ModelChoice::BgeSmallEnV15Q => {
                "Represent this sentence for searching relevant passages: "
            }
            ModelChoice::MultilingualE5Small => "query: ",
        }
    }

    fn passage_prefix(self) -> &'static str {
        match self {
            ModelChoice::BgeSmallEnV15Q => "",
            ModelChoice::MultilingualE5Small => "passage: ",
        }
    }
}

/// Model download progress. Only emitted when something actually has to be
/// downloaded; a cached model loads silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadEvent {
    /// About to download `total_bytes` (sum over all missing files).
    Started { total_bytes: u64 },
    /// Bytes downloaded so far, across all files. Emitted about every 256 KiB.
    Progress { done_bytes: u64, total_bytes: u64 },
    /// Every file is in the cache.
    Finished { total_bytes: u64 },
}

pub type DownloadCallback = Box<dyn Fn(DownloadEvent) + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("model download failed ({file}): {message}")]
    Download { file: String, message: String },
    #[error("could not read model file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not load the embedding model: {0}")]
    Model(#[from] fastembed::Error),
    #[error("embedding model self-check failed: {0}")]
    SelfCheck(String),
}

/// Tuning knobs; the defaults are right for the app.
#[derive(Debug, Clone, Default)]
pub struct EmbedOptions {
    /// ONNX Runtime intra-op threads. `None` = one per core.
    pub intra_threads: Option<usize>,
}

/// Texts per ONNX run. Each run pads to its longest input, and inputs are
/// length-sorted first, so this mostly bounds peak memory.
const BATCH_SIZE: usize = 64;
/// Token limit per input. A conversation window is ~900 chars (~250 tokens),
/// so 512 (the models' own limit) never truncates a real window.
const MAX_TOKENS: usize = 512;

pub struct FastEmbedder {
    choice: ModelChoice,
    /// `TextEmbedding::embed` takes `&mut self`; ORT parallelizes inside a run,
    /// so serializing callers costs nothing in throughput.
    model: Mutex<TextEmbedding>,
}

impl FastEmbedder {
    /// Load `model` from `cache_dir`, downloading it first if needed
    /// (blocking; run it off the UI thread). `cache_dir` is created if missing.
    pub fn new(
        model: ModelChoice,
        cache_dir: &Path,
        on_download: Option<DownloadCallback>,
    ) -> Result<Self, EmbedError> {
        Self::with_options(model, cache_dir, on_download, EmbedOptions::default())
    }

    pub fn with_options(
        model: ModelChoice,
        cache_dir: &Path,
        on_download: Option<DownloadCallback>,
        options: EmbedOptions,
    ) -> Result<Self, EmbedError> {
        let files = ensure_files(model, cache_dir, on_download.as_deref())?;
        let read = |p: &PathBuf| {
            std::fs::read(p).map_err(|source| EmbedError::Io {
                path: p.clone(),
                source,
            })
        };
        let tokenizer_files = TokenizerFiles {
            tokenizer_file: read(&files.tokenizer)?,
            config_file: read(&files.config)?,
            special_tokens_map_file: read(&files.special_tokens_map)?,
            tokenizer_config_file: read(&files.tokenizer_config)?,
        };
        let fe_model = model.fastembed_model();
        let pooling = TextEmbedding::get_default_pooling_method(&fe_model)
            .ok_or_else(|| EmbedError::SelfCheck(format!("no pooling known for {fe_model:?}")))?;
        let user_model = UserDefinedEmbeddingModel::new(read(&files.onnx)?, tokenizer_files)
            .with_pooling(pooling)
            .with_quantization(TextEmbedding::get_quantization_mode(&fe_model));
        let mut init = InitOptionsUserDefined::new().with_max_length(MAX_TOKENS);
        if let Some(n) = options.intra_threads {
            init = init.with_intra_threads(n);
        }
        let text_embedding = TextEmbedding::try_new_from_user_defined(user_model, init)?;

        let this = FastEmbedder {
            choice: model,
            model: Mutex::new(text_embedding),
        };
        // Fail at load time, not on the first search, if the files are wrong.
        let probe = this
            .embed_raw(&["ok".to_string()])
            .map_err(EmbedError::SelfCheck)?;
        if probe.len() != 1 || probe[0].len() != model.dims() {
            return Err(EmbedError::SelfCheck(format!(
                "expected {} dims, got {:?}",
                model.dims(),
                probe.first().map(Vec::len)
            )));
        }
        Ok(this)
    }

    pub fn choice(&self) -> ModelChoice {
        self.choice
    }

    /// Embed already-prefixed texts, preserving order. Inputs are sorted by
    /// length so each batch pads to similar lengths (chat windows vary from a
    /// few words to ~900 chars; unsorted batches waste most of their compute
    /// on padding).
    fn embed_raw(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| texts[i].len());
        let sorted: Vec<&str> = order.iter().map(|&i| texts[i].as_str()).collect();

        let vectors = {
            let mut model = self
                .model
                .lock()
                .map_err(|_| "embedder mutex poisoned".to_string())?;
            model
                .embed(&sorted, Some(BATCH_SIZE))
                .map_err(|e| e.to_string())?
        };
        if vectors.len() != texts.len() {
            return Err(format!(
                "got {} vectors for {} texts",
                vectors.len(),
                texts.len()
            ));
        }
        let mut out = vec![Vec::new(); texts.len()];
        for (vector, &i) in vectors.into_iter().zip(&order) {
            // fastembed L2-normalizes every pooled output (text_embedding/output.rs).
            debug_assert!((vector.iter().map(|x| x * x).sum::<f32>().sqrt() - 1.0).abs() < 1e-3);
            out[i] = vector;
        }
        Ok(out)
    }
}

impl Embedder for FastEmbedder {
    fn model_id(&self) -> &str {
        self.choice.model_id()
    }

    fn dims(&self) -> usize {
        self.choice.dims()
    }

    fn embed_passages(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let prefix = self.choice.passage_prefix();
        if prefix.is_empty() {
            self.embed_raw(texts)
        } else {
            let prefixed: Vec<String> = texts.iter().map(|t| format!("{prefix}{t}")).collect();
            self.embed_raw(&prefixed)
        }
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        let query = format!("{}{}", self.choice.query_prefix(), text);
        self.embed_raw(&[query])?
            .pop()
            .ok_or_else(|| "no vector returned".to_string())
    }

    /// Cosine distance (int8-quantized vectors) beyond which a window is
    /// unrelated. bge-small, measured on the fixture: the right conversation
    /// sits at 0.26 to 0.32, unrelated windows at 0.36 and up, and a nonsense
    /// query's best match at 0.395. e5's scores bunch together (all ~0.8
    /// similarity) and haven't been calibrated, so it keeps the default.
    fn max_distance(&self) -> f64 {
        match self.choice {
            ModelChoice::BgeSmallEnV15Q => 0.34,
            ModelChoice::MultilingualE5Small => 0.55,
        }
    }
}

// ------------------------------------------------------------ download ---

struct ModelFiles {
    onnx: PathBuf,
    tokenizer: PathBuf,
    config: PathBuf,
    special_tokens_map: PathBuf,
    tokenizer_config: PathBuf,
}

/// True when every file of `model` is already in `cache_dir` (no network).
pub fn is_cached(model: ModelChoice, cache_dir: &Path) -> bool {
    let cache = Cache::new(cache_dir.to_path_buf()).repo(repo(model));
    file_names(model).iter().all(|f| cache.get(f).is_some())
}

/// fastembed's (repo, ONNX file path) for `model`.
fn hf_location(model: ModelChoice) -> (String, String) {
    let fe_model = model.fastembed_model();
    let info = TextEmbedding::get_model_info(&fe_model)
        .expect("ModelChoice only maps to models fastembed lists");
    debug_assert!(info.additional_files.is_empty());
    (info.model_code.clone(), info.model_file.clone())
}

fn repo(model: ModelChoice) -> Repo {
    Repo::with_revision(
        hf_location(model).0,
        RepoType::Model,
        model.revision().to_string(),
    )
}

/// ONNX file first, then the four tokenizer files fastembed needs.
fn file_names(model: ModelChoice) -> [String; 5] {
    [
        hf_location(model).1,
        "tokenizer.json".into(),
        "config.json".into(),
        "special_tokens_map.json".into(),
        "tokenizer_config.json".into(),
    ]
}

fn ensure_files(
    model: ModelChoice,
    cache_dir: &Path,
    on_download: Option<&(dyn Fn(DownloadEvent) + Send + Sync)>,
) -> Result<ModelFiles, EmbedError> {
    let names = file_names(model);
    let cache = Cache::new(cache_dir.to_path_buf());
    let cached = cache.repo(repo(model));
    let missing: Vec<&String> = names.iter().filter(|f| cached.get(f).is_none()).collect();

    let mut paths: Vec<PathBuf> = Vec::with_capacity(names.len());
    if missing.is_empty() {
        paths.extend(names.iter().map(|f| cached.get(f).expect("checked above")));
    } else {
        std::fs::create_dir_all(cache_dir).map_err(|source| EmbedError::Io {
            path: cache_dir.to_path_buf(),
            source,
        })?;
        let download_err = |file: &str, e: hf_hub::api::sync::ApiError| EmbedError::Download {
            file: file.to_string(),
            message: e.to_string(),
        };
        let api = ApiBuilder::from_cache(cache)
            // Never send a token the user may have for their own HF account.
            .with_token(None)
            .with_progress(false)
            .with_retries(3)
            .build()
            .map_err(|e| download_err("(client)", e))?;
        let remote: ApiRepo = api.repo(repo(model));

        // Sizes up front (HEAD requests) so progress is against a real total.
        let mut sizes = Vec::with_capacity(missing.len());
        for f in &missing {
            let meta = api
                .metadata(&remote.url(f))
                .map_err(|e| download_err(f, e))?;
            sizes.push(meta.size() as u64);
        }
        let total: u64 = sizes.iter().sum();
        let emit = |e: DownloadEvent| {
            if let Some(cb) = on_download {
                cb(e)
            }
        };
        emit(DownloadEvent::Started { total_bytes: total });
        let mut done_before = 0u64;
        for (f, size) in missing.iter().zip(&sizes) {
            let progress = ByteProgress {
                emit: &emit,
                base: done_before,
                file_done: 0,
                last_emitted: 0,
                total,
            };
            remote
                .download_with_progress(f, progress)
                .map_err(|e| download_err(f, e))?;
            done_before += size;
        }
        emit(DownloadEvent::Finished { total_bytes: total });
        for f in &names {
            let path = cached.get(f).ok_or_else(|| EmbedError::Download {
                file: f.clone(),
                message: "not in cache after download".into(),
            })?;
            paths.push(path);
        }
    }
    // The model runs over the user's messages: never load bytes other than
    // the ones pinned here, whatever the network or cache delivered.
    for ((path, name), want) in paths.iter().zip(&names).zip(model.sha256s()) {
        let got = sha256_file(path)?;
        if got != want {
            return Err(EmbedError::Download {
                file: name.clone(),
                message: format!(
                    "checksum mismatch (expected {want}, got {got}); delete {} to re-download",
                    path.display()
                ),
            });
        }
    }
    let mut it = paths.into_iter();
    let mut next = || it.next().expect("five paths");
    Ok(ModelFiles {
        onnx: next(),
        tokenizer: next(),
        config: next(),
        special_tokens_map: next(),
        tokenizer_config: next(),
    })
}

fn sha256_file(path: &Path) -> Result<String, EmbedError> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|source| EmbedError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|source| EmbedError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Maps hf-hub's per-file progress onto one running total. hf-hub calls
/// `init` again (and `update(resume_offset)`) on every (re)try of a file, so
/// `init` resets the per-file count.
struct ByteProgress<'a, F: Fn(DownloadEvent)> {
    emit: &'a F,
    base: u64,
    file_done: u64,
    last_emitted: u64,
    total: u64,
}

impl<F: Fn(DownloadEvent)> Progress for ByteProgress<'_, F> {
    fn init(&mut self, _size: usize, _filename: &str) {
        self.file_done = 0;
    }

    fn update(&mut self, size: usize) {
        self.file_done += size as u64;
        let done = (self.base + self.file_done).min(self.total);
        if done - self.last_emitted.min(done) >= 256 * 1024 {
            self.last_emitted = done;
            (self.emit)(DownloadEvent::Progress {
                done_bytes: done,
                total_bytes: self.total,
            });
        }
    }

    fn finish(&mut self) {
        let done = (self.base + self.file_done).min(self.total);
        self.last_emitted = done;
        (self.emit)(DownloadEvent::Progress {
            done_bytes: done,
            total_bytes: self.total,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cos(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    /// Downloads the model (~67 MB) into `MS_EMBED_TEST_CACHE`.
    /// `MS_EMBED_TEST_CACHE=/some/dir cargo test -p ms-embed -- --ignored`
    #[test]
    #[ignore = "downloads the model"]
    fn paraphrases_rank_first() {
        let dir = std::env::var("MS_EMBED_TEST_CACHE")
            .expect("set MS_EMBED_TEST_CACHE to a scratch dir for the model files");
        let e = FastEmbedder::new(ModelChoice::default(), Path::new(&dir), None).unwrap();
        assert!(is_cached(ModelChoice::default(), Path::new(&dir)));
        assert_eq!(e.model_id(), "bge-small-en-v1.5-q");
        let passages: Vec<String> = [
            "the gate code is 4471# btw",
            "landing at 6:40pm, flight UA 1423 from Denver",
            "can you grab oat milk and eggs on your way home",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let pv = e.embed_passages(&passages).unwrap();
        for v in &pv {
            assert_eq!(v.len(), 384);
            assert!((cos(v, v) - 1.0).abs() < 1e-3);
        }
        for (q, want) in [
            ("how do I get through the gate", 0),
            ("when does the plane land", 1),
            ("groceries", 2),
        ] {
            let qv = e.embed_query(q).unwrap();
            let best = (0..pv.len())
                .max_by(|&a, &b| cos(&qv, &pv[a]).total_cmp(&cos(&qv, &pv[b])))
                .unwrap();
            assert_eq!(best, want, "query {q:?}");
        }
        // Order is preserved despite the internal length sort.
        let again = e.embed_passages(&passages[1..2]).unwrap();
        assert!(cos(&again[0], &pv[1]) > 0.999);
    }
}
