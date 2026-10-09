use crate::chunking::{Chunk, INTERIM_CHUNKER_VERSION};
use crate::contracts::{
    DocumentRecord, EmbeddingSpace, ModelDescriptor, ModelRole, ProviderErrorCode,
};
use crate::error::{CoreError, CoreResult};
use ort::{session::Session, value::Tensor};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tokenizers::tokenizer::Tokenizer;
use tokenizers::utils::padding::PaddingParams;
use tokenizers::utils::truncation::{TruncationParams, TruncationStrategy};

pub const DEFAULT_MAX_TOKENS: usize = 512;
pub const DEFAULT_BATCH_SIZE: usize = 16;
pub const E5_DIMENSIONS: usize = 384;
pub const EMBEDDING_IDLE_UNLOAD: Duration = Duration::from_secs(5 * 60);
/// Passages are embedded with their document title and path words, so a
/// query naming a document ("plano ng proyekto", "project plan") can match
/// it across languages. Part of the embedding-space fingerprint.
pub const PASSAGE_CONTEXT_VERSION: &str = "title-path-v1";
/// Persisted chunks are embedded from their stored text only. This is a
/// separate input contract from the snapshot path, which adds title and path
/// context before embedding.
pub const STORED_CHUNK_INPUT_VERSION: &str = "chunk-text-v1";

/// Derive the embedding space used by vectors persisted for native index
/// chunks. The provider identity is retained, while the input contract is
/// namespaced so stored vectors can never be compared with snapshot vectors.
pub fn stored_chunk_space(provider: &EmbeddingSpace) -> EmbeddingSpace {
    let mut hasher = Sha256::new();
    hasher.update(b"folio-stored-chunk\n");
    hasher.update(STORED_CHUNK_INPUT_VERSION.as_bytes());
    hasher.update(b"\n");
    hasher.update(provider.preprocessing_fingerprint.as_bytes());
    EmbeddingSpace {
        model_id: provider.model_id.clone(),
        revision: provider.revision.clone(),
        quantization: provider.quantization.clone(),
        dimensions: provider.dimensions,
        preprocessing_fingerprint: hex::encode(hasher.finalize()),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct E5SpaceInputs {
    pub model_id: String,
    pub revision: String,
    pub quantization: String,
    pub dimensions: usize,
    pub model_sha256: String,
    pub tokenizer_sha256: String,
    pub max_tokens: usize,
}

/// Construct the provider space from the descriptor metadata used by the
/// native E5 provider. Persisted chunk vectors are derived from this space by
/// `stored_chunk_space`; keeping this constructor shared prevents a resolver
/// from accidentally selecting the interim title/path snapshot space.
pub fn e5_provider_space(inputs: &E5SpaceInputs) -> EmbeddingSpace {
    EmbeddingSpace {
        model_id: inputs.model_id.clone(),
        revision: inputs.revision.clone(),
        quantization: inputs.quantization.clone(),
        dimensions: inputs.dimensions,
        preprocessing_fingerprint: embedding_fingerprint(
            &inputs.model_sha256,
            &inputs.tokenizer_sha256,
            "query: ",
            "passage: ",
            inputs.max_tokens,
            &format!("{INTERIM_CHUNKER_VERSION}+{PASSAGE_CONTEXT_VERSION}"),
        ),
    }
}

/// What the E5 provider needs from a model descriptor: the space inputs plus
/// the descriptor-relative file paths it loads. The provider construction site
/// and the persistent-space resolver both call this, so the `.onnx` /
/// `tokenizer.json` selection rule, the dimensions and the token limit exist
/// once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct E5DescriptorFiles {
    pub inputs: E5SpaceInputs,
    pub model_file: String,
    pub tokenizer_file: String,
}

pub fn e5_inputs_from_descriptor(descriptor: &ModelDescriptor) -> CoreResult<E5DescriptorFiles> {
    let corrupt = |message: &str| {
        CoreError::from(
            crate::error::NativeProviderErrorError::new(ProviderErrorCode::ModelCorrupt, message)
                .with_detail(descriptor.id.clone()),
        )
    };
    if !matches!(descriptor.role, ModelRole::Embedding) {
        return Err(corrupt("The selected model is not an embedding model."));
    }
    let model_file = descriptor
        .files
        .iter()
        .find(|file| file.path.ends_with(".onnx"))
        .ok_or_else(|| corrupt("The selected embedding model has no ONNX file."))?;
    let tokenizer_file = descriptor
        .files
        .iter()
        .find(|file| file.path.ends_with("tokenizer.json"))
        .ok_or_else(|| corrupt("The selected embedding model has no tokenizer file."))?;
    Ok(E5DescriptorFiles {
        inputs: E5SpaceInputs {
            model_id: descriptor.id.clone(),
            revision: descriptor.revision.clone(),
            quantization: descriptor.quantization.clone(),
            dimensions: E5_DIMENSIONS,
            model_sha256: model_file.sha256.clone(),
            tokenizer_sha256: tokenizer_file.sha256.clone(),
            max_tokens: DEFAULT_MAX_TOKENS,
        },
        model_file: model_file.path.clone(),
        tokenizer_file: tokenizer_file.path.clone(),
    })
}

/// The text embedded for one passage: document title, path words, then the
/// passage. Only the embedding input changes; source offsets, citations and
/// displayed text remain the original passage.
pub fn passage_embedding_text(document: Option<&DocumentRecord>, chunk: &Chunk) -> String {
    let Some(document) = document else {
        return chunk.text.clone();
    };
    let stem = document
        .relative_path
        .rsplit_once('.')
        .map_or(document.relative_path.as_str(), |(stem, _)| stem);
    let path_words = stem
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    format!("{}\n{}\n{}", document.title.trim(), path_words, chunk.text)
}

/// Embedding inputs for `chunks`, aligned by index.
pub fn passage_embedding_texts(documents: &[DocumentRecord], chunks: &[Chunk]) -> Vec<String> {
    chunks
        .iter()
        .map(|chunk| {
            let document = documents
                .iter()
                .find(|document| document.id == chunk.document_id);
            passage_embedding_text(document, chunk)
        })
        .collect()
}

/// Markdown `# ` heading, or the given fallback name.
pub fn markdown_title(name: &str, content: &str) -> String {
    content
        .lines()
        .find_map(|line| {
            line.strip_prefix("# ")
                .map(str::trim)
                .filter(|title| !title.is_empty())
        })
        .unwrap_or(name)
        .to_owned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmbeddingKind {
    Query,
    Passage,
}

#[derive(Clone, Debug)]
pub struct QueryEmbedding {
    pub space: EmbeddingSpace,
    pub vector: Vec<f32>,
}

pub trait EmbeddingProvider: Send {
    fn space(&self) -> &EmbeddingSpace;
    fn embed(
        &self,
        texts: &[String],
        kind: EmbeddingKind,
        cancel: Option<&AtomicBool>,
    ) -> CoreResult<Vec<Vec<f32>>>;

    fn embed_query(&self, text: &str, cancel: Option<&AtomicBool>) -> CoreResult<QueryEmbedding> {
        let vectors = self.embed(&[text.to_owned()], EmbeddingKind::Query, cancel)?;
        if vectors.len() != 1 {
            return Err(CoreError::Message(
                "The embedding provider must return exactly one query vector.".into(),
            ));
        }
        let vector = vectors.into_iter().next().expect("length checked above");
        Ok(QueryEmbedding {
            space: self.space().clone(),
            vector,
        })
    }

    fn unload(&self) -> CoreResult<()>;
}

pub struct OrtE5Provider {
    space: EmbeddingSpace,
    tokenizer: Tokenizer,
    /// Dropped by the idle reaper and reloaded on the next use, from the same
    /// verified model path.
    session: Arc<Mutex<Option<Session>>>,
    model_path: PathBuf,
    threads: usize,
    batch_size: usize,
    active: Arc<AtomicBool>,
    last_used: Arc<Mutex<Instant>>,
    reaper_stop: Arc<AtomicBool>,
    reaper: Option<thread::JoinHandle<()>>,
}

impl OrtE5Provider {
    #[allow(clippy::too_many_arguments)]
    pub fn from_files(
        model_path: impl AsRef<Path>,
        tokenizer_path: impl AsRef<Path>,
        model_id: impl Into<String>,
        revision: impl Into<String>,
        quantization: impl Into<String>,
        dimensions: usize,
        model_sha256: &str,
        tokenizer_sha256: &str,
        max_tokens: usize,
        batch_size: usize,
        threads: usize,
    ) -> CoreResult<Self> {
        Self::from_files_with_idle(
            model_path,
            tokenizer_path,
            model_id,
            revision,
            quantization,
            dimensions,
            model_sha256,
            tokenizer_sha256,
            max_tokens,
            batch_size,
            threads,
            EMBEDDING_IDLE_UNLOAD,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_files_with_idle(
        model_path: impl AsRef<Path>,
        tokenizer_path: impl AsRef<Path>,
        model_id: impl Into<String>,
        revision: impl Into<String>,
        quantization: impl Into<String>,
        dimensions: usize,
        model_sha256: &str,
        tokenizer_sha256: &str,
        max_tokens: usize,
        batch_size: usize,
        threads: usize,
        idle_unload: Duration,
    ) -> CoreResult<Self> {
        if max_tokens == 0 || batch_size == 0 || dimensions == 0 {
            return Err(CoreError::Message(
                "Embedding limits and dimensions must be positive.".into(),
            ));
        }
        let space = e5_provider_space(&E5SpaceInputs {
            model_id: model_id.into(),
            revision: revision.into(),
            quantization: quantization.into(),
            dimensions,
            model_sha256: model_sha256.into(),
            tokenizer_sha256: tokenizer_sha256.into(),
            max_tokens,
        });
        initialize_ort(threads)?;
        let mut tokenizer = Tokenizer::from_file(tokenizer_path.as_ref())
            .map_err(|error| CoreError::Message(format!("Tokenizer load failed: {error}")))?;
        tokenizer.with_padding(Some(PaddingParams::default()));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: max_tokens,
                strategy: TruncationStrategy::LongestFirst,
                ..Default::default()
            }))
            .map_err(|error| CoreError::Message(format!("Tokenizer setup failed: {error}")))?;
        let model_path = model_path.as_ref().to_path_buf();
        let session = load_session(&model_path, threads)?;
        let session = Arc::new(Mutex::new(Some(session)));
        let active = Arc::new(AtomicBool::new(false));
        let last_used = Arc::new(Mutex::new(Instant::now()));
        let reaper_stop = Arc::new(AtomicBool::new(false));
        let reaper = Some(spawn_idle_reaper(
            session.clone(),
            active.clone(),
            last_used.clone(),
            reaper_stop.clone(),
            idle_unload,
        ));
        Ok(Self {
            space,
            tokenizer,
            session,
            model_path,
            threads,
            batch_size,
            active,
            last_used,
            reaper_stop,
            reaper,
        })
    }

    fn encode_batch(
        &self,
        texts: &[String],
        kind: EmbeddingKind,
    ) -> CoreResult<(Vec<Vec<i64>>, Vec<Vec<i64>>)> {
        let prefix = match kind {
            EmbeddingKind::Query => "query: ",
            EmbeddingKind::Passage => "passage: ",
        };
        let inputs: Vec<String> = texts.iter().map(|text| format!("{prefix}{text}")).collect();
        let encodings = self
            .tokenizer
            .encode_batch(inputs, true)
            .map_err(|error| CoreError::Message(format!("Tokenizer encoding failed: {error}")))?;
        Ok(encodings
            .into_iter()
            .map(|encoding| {
                (
                    encoding.get_ids().iter().map(|id| *id as i64).collect(),
                    encoding
                        .get_attention_mask()
                        .iter()
                        .map(|mask| *mask as i64)
                        .collect(),
                )
            })
            .unzip())
    }

    fn run_batch(&self, ids: Vec<Vec<i64>>, masks: Vec<Vec<i64>>) -> CoreResult<Vec<Vec<f32>>> {
        let batch = ids.len();
        let sequence = ids.first().map_or(0, Vec::len);
        if batch == 0 || sequence == 0 || ids.iter().any(|row| row.len() != sequence) {
            return Err(CoreError::Message(
                "Tokenizer produced an invalid batch.".into(),
            ));
        }
        let flat_ids = ids.iter().flatten().copied().collect::<Vec<_>>();
        let flat_masks = masks.iter().flatten().copied().collect::<Vec<_>>();
        let mut session_guard = self
            .session
            .lock()
            .map_err(|_| CoreError::Message("Embedding session is unavailable.".into()))?;
        let session = ensure_loaded(&mut *session_guard, || {
            load_session(&self.model_path, self.threads)
        })?;
        let mut inputs = Vec::new();
        for input in session.inputs() {
            let name = input.name();
            let values = match name {
                "input_ids" | "input_ids:0" => flat_ids.clone(),
                "attention_mask" | "attention_mask:0" => flat_masks.clone(),
                "token_type_ids" | "token_type_ids:0" => vec![0_i64; batch * sequence],
                other => {
                    return Err(CoreError::Message(format!(
                        "Unsupported E5 model input: {other}"
                    )))
                }
            };
            let tensor = Tensor::from_array(([batch, sequence], values))
                .map_err(|error| CoreError::Message(format!("ONNX input setup failed: {error}")))?;
            inputs.push((name.to_owned(), tensor));
        }
        let outputs = session
            .run(inputs)
            .map_err(|error| CoreError::Message(format!("ONNX inference failed: {error}")))?;
        let output = outputs
            .values()
            .next()
            .ok_or_else(|| CoreError::Message("E5 model returned no output.".into()))?;
        let (shape, values) = output.try_extract_tensor::<f32>().map_err(|error| {
            CoreError::Message(format!("ONNX output extraction failed: {error}"))
        })?;
        let shape = shape.to_vec();
        let values = values.to_vec();
        if shape.len() == 2 {
            if shape[0] as usize != batch || shape[1] as usize != self.space.dimensions {
                return Err(CoreError::Message(
                    "E5 pooled output has unexpected dimensions.".into(),
                ));
            }
            return Ok(values
                .chunks_exact(self.space.dimensions)
                .map(normalize)
                .collect());
        }
        if shape.len() != 3
            || shape[0] as usize != batch
            || shape[1] as usize != sequence
            || shape[2] as usize != self.space.dimensions
        {
            return Err(CoreError::Message(
                "E5 output has unexpected dimensions.".into(),
            ));
        }
        let mut result = Vec::with_capacity(batch);
        for row in 0..batch {
            let mut pooled = vec![0.0; self.space.dimensions];
            let mut count = 0.0;
            for token in 0..sequence {
                if masks[row][token] == 0 {
                    continue;
                }
                count += 1.0;
                let offset = (row * sequence + token) * self.space.dimensions;
                for (dimension, value) in pooled.iter_mut().enumerate() {
                    *value += values[offset + dimension];
                }
            }
            if count > 0.0 {
                for value in &mut pooled {
                    *value /= count;
                }
            }
            result.push(normalize(&pooled));
        }
        Ok(result)
    }
}

impl EmbeddingProvider for OrtE5Provider {
    fn space(&self) -> &EmbeddingSpace {
        &self.space
    }

    fn embed(
        &self,
        texts: &[String],
        kind: EmbeddingKind,
        cancel: Option<&AtomicBool>,
    ) -> CoreResult<Vec<Vec<f32>>> {
        self.active.store(true, Ordering::Release);
        let _active = EmbeddingActiveGuard(&self.active);
        if let Ok(mut last_used) = self.last_used.lock() {
            *last_used = Instant::now();
        }
        let mut output = Vec::with_capacity(texts.len());
        for batch in texts.chunks(self.batch_size) {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                return Err(CoreError::Provider(
                    crate::error::NativeProviderErrorError::new(
                        crate::contracts::ProviderErrorCode::Cancelled,
                        "Embedding cancelled.",
                    ),
                ));
            }
            let (ids, masks) = self.encode_batch(batch, kind)?;
            output.extend(self.run_batch(ids, masks)?);
        }
        Ok(output)
    }

    fn unload(&self) -> CoreResult<()> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| CoreError::Message("Embedding session is unavailable.".into()))?;
        session.take();
        Ok(())
    }
}

impl Drop for OrtE5Provider {
    fn drop(&mut self) {
        self.reaper_stop.store(true, Ordering::Release);
        if let Some(reaper) = self.reaper.take() {
            let _ = reaper.join();
        }
        let _ = self.unload();
    }
}

fn load_session(model_path: &Path, threads: usize) -> CoreResult<Session> {
    Session::builder()
        .map_err(|error| CoreError::Message(format!("ONNX session setup failed: {error}")))?
        .with_intra_threads(threads.clamp(1, 2))
        .map_err(|error| CoreError::Message(format!("ONNX thread setup failed: {error}")))?
        .commit_from_file(model_path)
        .map_err(|error| CoreError::Message(format!("ONNX model load failed: {error}")))
}

/// The loaded value, loading it first if the idle reaper dropped it.
fn ensure_loaded<T>(
    slot: &mut Option<T>,
    load: impl FnOnce() -> CoreResult<T>,
) -> CoreResult<&mut T> {
    if slot.is_none() {
        *slot = Some(load()?);
    }
    Ok(slot.as_mut().expect("loaded above"))
}

fn spawn_idle_reaper<T: Send + 'static>(
    session: Arc<Mutex<Option<T>>>,
    active: Arc<AtomicBool>,
    last_used: Arc<Mutex<Instant>>,
    stop: Arc<AtomicBool>,
    idle_unload: Duration,
) -> thread::JoinHandle<()> {
    let interval = idle_unload
        .min(Duration::from_millis(100))
        .max(Duration::from_millis(10));
    thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            thread::sleep(interval);
            if stop.load(Ordering::Acquire) || active.load(Ordering::Acquire) {
                continue;
            }
            let expired = last_used
                .lock()
                .ok()
                .is_some_and(|last_used| last_used.elapsed() >= idle_unload);
            if expired {
                if let Ok(mut session) = session.lock() {
                    session.take();
                }
            }
        }
    })
}

struct EmbeddingActiveGuard<'a>(&'a AtomicBool);

impl Drop for EmbeddingActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn normalize(values: &[f32]) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm == 0.0 {
        return values.to_vec();
    }
    values.iter().map(|value| value / norm).collect()
}

fn initialize_ort(threads: usize) -> CoreResult<()> {
    static READY: OnceLock<bool> = OnceLock::new();
    READY.get_or_init(|| {
        let _ = threads;
        ort::init().with_name("folio").commit()
    });
    if READY.get().copied().unwrap_or(false) {
        Ok(())
    } else {
        Err(CoreError::Message(
            "ONNX Runtime initialization failed.".into(),
        ))
    }
}

#[derive(Serialize)]
struct FingerprintInput<'a> {
    model_sha256: &'a str,
    tokenizer_sha256: &'a str,
    query_prefix: &'a str,
    passage_prefix: &'a str,
    max_tokens: usize,
    chunker_version: &'a str,
    pooling: &'static str,
    normalization: &'static str,
}

pub fn embedding_fingerprint(
    model_sha256: &str,
    tokenizer_sha256: &str,
    query_prefix: &str,
    passage_prefix: &str,
    max_tokens: usize,
    chunker_version: &str,
) -> String {
    let input = FingerprintInput {
        model_sha256,
        tokenizer_sha256,
        query_prefix,
        passage_prefix,
        max_tokens,
        chunker_version,
        pooling: "attention-masked-mean",
        normalization: "l2",
    };
    let bytes = serde_json::to_vec(&input).expect("fingerprint input is serializable");
    hex::encode(Sha256::digest(bytes))
}

pub fn normalize_vector(values: &[f32]) -> Vec<f32> {
    normalize(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_unloaded_value_is_reloaded_on_the_next_use() {
        let slot = Arc::new(Mutex::new(Some(1_u32)));
        let active = Arc::new(AtomicBool::new(false));
        let last_used = Arc::new(Mutex::new(Instant::now()));
        let stop = Arc::new(AtomicBool::new(false));
        let reaper = spawn_idle_reaper(
            slot.clone(),
            active,
            last_used,
            stop.clone(),
            Duration::from_millis(20),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while slot.lock().unwrap().is_some() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        stop.store(true, Ordering::Release);
        reaper.join().unwrap();
        assert!(
            slot.lock().unwrap().is_none(),
            "the reaper unloaded the value"
        );

        let mut loads = 0;
        let mut guard = slot.lock().unwrap();
        let value = ensure_loaded(&mut *guard, || {
            loads += 1;
            Ok(7_u32)
        })
        .unwrap();
        assert_eq!(*value, 7);
        let value = ensure_loaded(&mut *guard, || {
            loads += 1;
            Ok(8_u32)
        })
        .unwrap();
        assert_eq!(*value, 7, "a loaded value is reused, not reloaded");
        assert_eq!(loads, 1);
    }

    #[test]
    fn fingerprint_changes_for_model_inputs_and_chunker_revision() {
        let first = embedding_fingerprint("model", "tokenizer", "query: ", "passage: ", 512, "v1");
        assert_ne!(
            first,
            embedding_fingerprint("model-2", "tokenizer", "query: ", "passage: ", 512, "v1")
        );
        assert_ne!(
            first,
            embedding_fingerprint("model", "tokenizer", "query: ", "passage: ", 256, "v1")
        );
        assert_ne!(
            first,
            embedding_fingerprint("model", "tokenizer", "query: ", "passage: ", 512, "v2")
        );
    }

    #[test]
    fn e5_provider_space_has_a_pinned_descriptor_derived_fingerprint() {
        let space = e5_provider_space(&E5SpaceInputs {
            model_id: "e5-small".into(),
            revision: "revision".into(),
            quantization: "int8".into(),
            dimensions: E5_DIMENSIONS,
            model_sha256: "sha256:model".into(),
            tokenizer_sha256: "sha256:tokenizer".into(),
            max_tokens: DEFAULT_MAX_TOKENS,
        });

        assert_eq!(
            space.preprocessing_fingerprint,
            "cf00c0efc0cc14902f610048e28e2b1c1eb90452e22c85844652cd3a827b30aa"
        );
        assert_eq!(
            stored_chunk_space(&space).preprocessing_fingerprint,
            "e72ccaa8652657ee0191868c230f713085c44a46ba8767836484a417c9872e22"
        );
    }

    #[test]
    fn stored_chunk_space_is_separate_and_tracks_provider_identity() {
        let provider = EmbeddingSpace {
            model_id: "model".into(),
            revision: "revision".into(),
            quantization: "int8".into(),
            dimensions: 384,
            preprocessing_fingerprint: "snapshot-input".into(),
        };
        let stored = stored_chunk_space(&provider);
        assert_eq!(stored.model_id, provider.model_id);
        assert_eq!(stored.revision, provider.revision);
        assert_eq!(stored.quantization, provider.quantization);
        assert_eq!(stored.dimensions, provider.dimensions);
        assert_ne!(
            stored.preprocessing_fingerprint,
            provider.preprocessing_fingerprint
        );
        assert_eq!(stored, stored_chunk_space(&provider));

        let mut changed = provider.clone();
        changed.revision = "next-revision".into();
        assert_ne!(stored, stored_chunk_space(&changed));
    }

    #[test]
    fn passage_embedding_text_adds_title_and_path_words_only_to_the_input() {
        let document = DocumentRecord {
            id: "w:projects/project-plan.md".into(),
            workspace_id: "w".into(),
            relative_path: "projects/project-plan.md".into(),
            name: "project-plan.md".into(),
            title: "Community Learning Project".into(),
            language: crate::contracts::Language::En,
            media_type: "text/markdown".into(),
            size_bytes: 8,
            modified_at_ms: None,
            content: None,
            content_hash: None,
        };
        let chunk = Chunk {
            document_id: "w:projects/project-plan.md".into(),
            ordinal: 0,
            text: "Due soon".into(),
            start: 0,
            end: 8,
            content_hash: "sha256:00".into(),
            page: None,
        };
        assert_eq!(
            passage_embedding_text(Some(&document), &chunk),
            "Community Learning Project\nprojects project plan\nDue soon"
        );
        assert_eq!(passage_embedding_text(None, &chunk), "Due soon");
        assert_eq!(
            passage_embedding_texts(&[document], std::slice::from_ref(&chunk)),
            vec!["Community Learning Project\nprojects project plan\nDue soon".to_owned()]
        );
        assert_eq!(markdown_title("x.md", "intro\n# Heading \nbody"), "Heading");
        assert_eq!(markdown_title("x.md", "no heading"), "x.md");
    }

    #[test]
    fn normalization_is_l2() {
        let vector = normalize_vector(&[3.0, 4.0]);
        assert!((vector[0] - 0.6).abs() < 0.0001);
        assert!((vector[1] - 0.8).abs() < 0.0001);
    }

    #[test]
    #[ignore = "requires verified FOLIO_E5_MODEL and FOLIO_E5_TOKENIZER files"]
    fn real_e5_embedding_smoke_uses_the_local_onnx_provider() {
        let model_path = std::env::var("FOLIO_E5_MODEL").expect("FOLIO_E5_MODEL");
        let tokenizer_path = std::env::var("FOLIO_E5_TOKENIZER").expect("FOLIO_E5_TOKENIZER");
        let model_sha256 = std::env::var("FOLIO_E5_MODEL_SHA256").expect("FOLIO_E5_MODEL_SHA256");
        let tokenizer_sha256 =
            std::env::var("FOLIO_E5_TOKENIZER_SHA256").expect("FOLIO_E5_TOKENIZER_SHA256");
        let provider = OrtE5Provider::from_files(
            model_path,
            tokenizer_path,
            "multilingual-e5-small-int8",
            "761b726dd34fb83930e26aab4e9ac3899aa1fa78",
            "int8",
            384,
            &model_sha256,
            &tokenizer_sha256,
            DEFAULT_MAX_TOKENS,
            DEFAULT_BATCH_SIZE,
            2,
        )
        .expect("local E5 provider loads");
        let values = provider
            .embed(
                &["Ang deadline ay sa Biyernes.".into()],
                EmbeddingKind::Passage,
                None,
            )
            .expect("local E5 provider embeds");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].len(), 384);
        let norm = values[0]
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }
}
