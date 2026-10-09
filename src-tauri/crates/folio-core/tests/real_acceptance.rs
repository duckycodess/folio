use folio_core::chunking::{Chunk, ChunkSource, InterimTextChunker, TextDocument};
use folio_core::contracts::{
    DocumentRecord, GroundedAnswerKind, InterpretationResult, Language, ModelDescriptor, ModelFile,
    OperationProposal, ProviderErrorCode, SearchResult,
};
use folio_core::embeddings::{EmbeddingKind, EmbeddingProvider, OrtE5Provider};
use folio_core::error::{CoreError, CoreResult};
use folio_core::generation::{
    ChatMessage, GenerationBudget, GenerationProvider, LlamaServerProvider,
};
use folio_core::grounding;
use folio_core::interpretation;
use folio_core::retrieval::HybridRetriever;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const FIXTURE_PATHS: &[&str] = &[
    "archive/project-plan-copy.md",
    "courses/math-review.md",
    "courses/pagsasanay-sa-math.md",
    "meetings/meeting-notes.md",
    "notes/paalala.md",
    "notes/study-session.md",
    "notes/tala-sa-proyekto.md",
    "personal/budget-notes.md",
    "personal/grocery-list.md",
    "personal/travel-notes.md",
    "projects/project-plan.md",
    "projects/submission-checklist.md",
    "research/methodology-notes.md",
    "research/review-reminders.md",
    "research/tala-sa-pamamaraan.md",
];

const E5_MODEL_ID: &str = "multilingual-e5-small-int8";
const QWEN_MODEL_ID: &str = "qwen3-0.6b-q4-k-m";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/documents")
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/model-manifest.json")
}

fn manifest() -> Value {
    serde_json::from_str(
        &fs::read_to_string(manifest_path()).expect("model manifest exists for R8"),
    )
    .expect("model manifest is valid JSON")
}

fn manifest_model(manifest: &Value, id: &str) -> ModelDescriptor {
    let model = manifest["models"]
        .as_array()
        .and_then(|models| models.iter().find(|model| model["id"].as_str() == Some(id)))
        .cloned()
        .unwrap_or_else(|| panic!("model {id} is present in the manifest"));
    serde_json::from_value(model).expect("manifest model has the native descriptor shape")
}

fn manifest_file(descriptor: &ModelDescriptor, path: &str) -> ModelFile {
    descriptor
        .files
        .iter()
        .find(|file| file.path == path)
        .cloned()
        .unwrap_or_else(|| panic!("{path} is present in model {}", descriptor.id))
}

fn sha256_file(path: &Path) -> String {
    let mut file =
        File::open(path).unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    hex::encode(hasher.finalize())
}

fn verify_model_file(label: &str, path: &Path, expected: &ModelFile) {
    let bytes = fs::metadata(path)
        .unwrap_or_else(|error| panic!("{label} metadata at {}: {error}", path.display()))
        .len();
    assert_eq!(
        bytes, expected.bytes,
        "{label} byte size differs from manifest"
    );
    assert_eq!(
        sha256_file(path),
        expected.sha256,
        "{label} SHA-256 differs from manifest"
    );
}

struct VerifiedInputs {
    embedding: ModelDescriptor,
    generation: ModelDescriptor,
    e5_model: PathBuf,
    e5_tokenizer: PathBuf,
    qwen_model: PathBuf,
    llama_server: PathBuf,
    e5_model_file: ModelFile,
    e5_tokenizer_file: ModelFile,
    qwen_model_file: ModelFile,
}

fn required_env_hash(name: &str, expected: &str) {
    let actual = std::env::var(name).unwrap_or_else(|_| panic!("{name}"));
    assert_eq!(actual, expected, "{name} differs from the pinned manifest");
}

fn verified_inputs() -> VerifiedInputs {
    let manifest = manifest();
    let embedding = manifest_model(&manifest, E5_MODEL_ID);
    let generation = manifest_model(&manifest, QWEN_MODEL_ID);
    let e5_model_file = manifest_file(&embedding, "onnx/model_quantized.onnx");
    let e5_tokenizer_file = manifest_file(&embedding, "tokenizer.json");
    let qwen_model_file = manifest_file(&generation, "Qwen3-0.6B-Q4_K_M.gguf");
    let e5_model = PathBuf::from(std::env::var("FOLIO_E5_MODEL").expect("FOLIO_E5_MODEL"));
    let e5_tokenizer =
        PathBuf::from(std::env::var("FOLIO_E5_TOKENIZER").expect("FOLIO_E5_TOKENIZER"));
    let qwen_model = PathBuf::from(std::env::var("FOLIO_QWEN_MODEL").expect("FOLIO_QWEN_MODEL"));
    let llama_server =
        PathBuf::from(std::env::var("FOLIO_LLAMA_SERVER").expect("FOLIO_LLAMA_SERVER"));
    required_env_hash("FOLIO_E5_MODEL_SHA256", &e5_model_file.sha256);
    required_env_hash("FOLIO_E5_TOKENIZER_SHA256", &e5_tokenizer_file.sha256);
    required_env_hash("FOLIO_QWEN_MODEL_SHA256", &qwen_model_file.sha256);
    verify_model_file("E5 model", &e5_model, &e5_model_file);
    verify_model_file("E5 tokenizer", &e5_tokenizer, &e5_tokenizer_file);
    verify_model_file("Qwen model", &qwen_model, &qwen_model_file);
    assert!(
        llama_server.is_file(),
        "FOLIO_LLAMA_SERVER points to a missing executable"
    );
    VerifiedInputs {
        embedding,
        generation,
        e5_model,
        e5_tokenizer,
        qwen_model,
        llama_server,
        e5_model_file,
        e5_tokenizer_file,
        qwen_model_file,
    }
}

fn fixture_corpus() -> (Vec<DocumentRecord>, HashMap<String, String>, Vec<Chunk>) {
    let mut sources = Vec::with_capacity(FIXTURE_PATHS.len());
    let mut contents = HashMap::with_capacity(FIXTURE_PATHS.len());
    for relative_path in FIXTURE_PATHS {
        let content = fs::read_to_string(fixture_root().join(relative_path))
            .unwrap_or_else(|error| panic!("read fixture {relative_path}: {error}"));
        let name = Path::new(relative_path)
            .file_name()
            .and_then(|value| value.to_str())
            .expect("fixture has a UTF-8 file name");
        let record = DocumentRecord {
            id: (*relative_path).into(),
            workspace_id: "fixtures".into(),
            relative_path: (*relative_path).into(),
            name: name.into(),
            title: folio_core::embeddings::markdown_title(name, &content),
            language: grounding::detect_language(&content),
            media_type: "text/markdown".into(),
            size_bytes: content.len() as u64,
            modified_at_ms: None,
            content: Some(content.clone()),
            content_hash: None,
        };
        contents.insert((*relative_path).into(), content.clone());
        sources.push(TextDocument::new(record, content));
    }
    let chunker = InterimTextChunker::new(sources);
    let documents = chunker.documents();
    let chunks = chunker.all_chunks().expect("fixture chunking succeeds");
    (documents, contents, chunks)
}

fn embedding_provider(inputs: &VerifiedInputs) -> OrtE5Provider {
    OrtE5Provider::from_files(
        &inputs.e5_model,
        &inputs.e5_tokenizer,
        inputs.embedding.id.clone(),
        inputs.embedding.revision.clone(),
        inputs.embedding.quantization.clone(),
        384,
        &inputs.e5_model_file.sha256,
        &inputs.e5_tokenizer_file.sha256,
        folio_core::embeddings::DEFAULT_MAX_TOKENS,
        folio_core::embeddings::DEFAULT_BATCH_SIZE,
        2,
    )
    .expect("local multilingual E5 provider loads")
}

fn generation_provider(inputs: &VerifiedInputs) -> LlamaServerProvider {
    LlamaServerProvider::from_verified_model(
        inputs.llama_server.clone(),
        folio_core::models::VerifiedModelFile {
            descriptor: inputs.generation.clone(),
            path: inputs.qwen_model.clone(),
        },
        2,
    )
    .expect("local llama provider loads")
}

struct GenerationGuard {
    provider: Arc<LlamaServerProvider>,
}

impl GenerationGuard {
    fn new(inputs: &VerifiedInputs) -> Self {
        Self {
            provider: Arc::new(generation_provider(inputs)),
        }
    }
}

impl Deref for GenerationGuard {
    type Target = LlamaServerProvider;

    fn deref(&self) -> &Self::Target {
        &self.provider
    }
}

impl Drop for GenerationGuard {
    fn drop(&mut self) {
        let _ = self.provider.unload();
    }
}

struct PreparedAcceptance {
    inputs: VerifiedInputs,
    documents: Vec<DocumentRecord>,
    contents: HashMap<String, String>,
    chunks: Vec<Chunk>,
    embeddings: Mutex<OrtE5Provider>,
    retriever: HybridRetriever,
}

static PREPARED: OnceLock<PreparedAcceptance> = OnceLock::new();

fn prepared() -> &'static PreparedAcceptance {
    PREPARED.get_or_init(|| {
        let inputs = verified_inputs();
        let (documents, contents, chunks) = fixture_corpus();
        let embeddings = embedding_provider(&inputs);
        let passage_texts = folio_core::embeddings::passage_embedding_texts(&documents, &chunks);
        let vectors = embeddings
            .embed(&passage_texts, EmbeddingKind::Passage, None)
            .expect("real E5 passage embeddings");
        println!(
            "R8 ort crate=2.0.0-rc.13 api_minor={} build_info={}",
            ort::MINOR_VERSION,
            ort::info()
        );
        let mut retriever = HybridRetriever::default();
        retriever
            .vector_index
            .replace(embeddings.space().clone(), chunks.clone(), vectors)
            .expect("fixture vectors fit the E5 space");
        PreparedAcceptance {
            inputs,
            documents,
            contents,
            chunks,
            embeddings: Mutex::new(embeddings),
            retriever,
        }
    })
}

fn retrieve(
    prepared: &PreparedAcceptance,
    query: &str,
    limit: usize,
) -> (Vec<SearchResult>, Vec<Value>) {
    let embeddings = prepared
        .embeddings
        .lock()
        .expect("embedding provider lock is available");
    let query_embedding = embeddings
        .embed_query(query, None)
        .unwrap_or_else(|error| panic!("embed query {query:?}: {error}"));
    let cosine_scores = prepared
        .retriever
        .vector_index
        .search(&query_embedding, limit)
        .unwrap_or_else(|error| panic!("score query {query:?}: {error}"))
        .into_iter()
        .map(|(chunk, score)| {
            json!({
                "documentId": chunk.document_id,
                "start": chunk.start,
                "end": chunk.end,
                "score": score,
            })
        })
        .collect();
    let results = prepared
        .retriever
        .search(
            &prepared.documents,
            &prepared.chunks,
            query,
            Some(&query_embedding),
            limit,
        )
        .unwrap_or_else(|error| panic!("retrieve query {query:?}: {error}"));
    (results, cosine_scores)
}

fn retrieval_evidence(results: &[SearchResult]) -> Vec<Value> {
    results
        .iter()
        .map(|result| {
            json!({
                "path": result.document.relative_path,
                "score": result.score,
                "method": &result.method,
                "passages": &result.passages,
            })
        })
        .collect()
}

fn output_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("FOLIO_R8_OUTPUT_DIR").expect("FOLIO_R8_OUTPUT_DIR is set by the workflow"),
    )
}

fn write_evidence(phase: &str, evidence: Value) {
    let directory = output_dir();
    fs::create_dir_all(&directory).expect("R8 evidence directory is writable");
    let path = directory.join(format!("{phase}.json"));
    fs::write(
        &path,
        serde_json::to_vec_pretty(&evidence).expect("R8 evidence serializes"),
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

fn assert_edit_proposal(result: &InterpretationResult) {
    match result {
        InterpretationResult::Proposal {
            proposal:
                OperationProposal::Edit {
                    document_id,
                    find,
                    replace,
                    ..
                },
            exact_duplicate_paths,
            ..
        } => {
            assert_eq!(document_id, "projects/project-plan.md");
            assert_eq!(find, "October 20");
            assert_eq!(replace, "October 23");
            assert!(exact_duplicate_paths
                .iter()
                .any(|path| path == "archive/project-plan-copy.md"));
        }
        other => panic!("expected deadline edit proposal, got {other:?}"),
    }
}

struct CountingProvider<'a> {
    inner: &'a dyn GenerationProvider,
    calls: AtomicUsize,
}

impl GenerationProvider for CountingProvider<'_> {
    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn revision(&self) -> &str {
        self.inner.revision()
    }

    fn generate_json(
        &self,
        schema: &Value,
        messages: &[ChatMessage],
        budget: &GenerationBudget,
        cancel: &AtomicBool,
    ) -> CoreResult<Value> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.generate_json(schema, messages, budget, cancel)
    }

    fn unload(&self) -> CoreResult<()> {
        self.inner.unload()
    }
}

fn provider_error_code(error: &CoreError) -> Option<ProviderErrorCode> {
    match error {
        CoreError::Provider(provider) => Some(provider.code.clone()),
        _ => None,
    }
}

fn result_evidence(result: &CoreResult<Value>) -> Value {
    match result {
        Ok(value) => json!({ "ok": value }),
        Err(error) => json!({
            "error": error.to_string(),
            "providerCode": provider_error_code(error).map(|code| format!("{code:?}")),
        }),
    }
}

fn assert_citations_within(
    summary: &folio_core::contracts::GroundedResult,
    supplied: &[folio_core::contracts::SourcePassage],
) {
    assert!(summary
        .sentences
        .iter()
        .all(|sentence| !sentence.citations.is_empty()));
    for sentence in &summary.sentences {
        for citation in &sentence.citations {
            assert!(
                supplied.iter().any(|passage| passage == citation),
                "summary citation is outside the supplied passages"
            );
        }
    }
}

#[test]
#[ignore = "requires verified local E5 and llama.cpp model files"]
fn r8_retrieval_cross_language() {
    let prepared = prepared();
    let english_query = "Community Learning Project submission deadline";
    let filipino_query = "Hanapin ang plano at huling araw ng pagpasa ng proyekto";
    let taglish_query = "Saan yung notes about consent ng interview participants?";
    let (english, english_scores) = retrieve(prepared, english_query, 5);
    let (filipino, filipino_scores) = retrieve(prepared, filipino_query, 5);
    let (taglish, taglish_scores) = retrieve(prepared, taglish_query, 5);
    write_evidence(
        "r8_retrieval_cross_language",
        json!({
            "inputs": {
                "englishToFilipino": english_query,
                "filipinoToEnglish": filipino_query,
                "taglish": taglish_query,
                "corpus": FIXTURE_PATHS,
            },
            "results": {
                "englishToFilipino": {
                    "rankedDocuments": retrieval_evidence(&english),
                    "topKCosineScores": english_scores,
                },
                "filipinoToEnglish": {
                    "rankedDocuments": retrieval_evidence(&filipino),
                    "topKCosineScores": filipino_scores,
                },
                "taglish": {
                    "rankedDocuments": retrieval_evidence(&taglish),
                    "topKCosineScores": taglish_scores,
                },
            },
        }),
    );
    assert!(english
        .iter()
        .any(|result| result.document.relative_path == "notes/tala-sa-proyekto.md"));
    assert!(!english
        .iter()
        .any(|result| result.document.relative_path == "personal/grocery-list.md"));
    assert!(filipino
        .iter()
        .any(|result| result.document.relative_path == "projects/project-plan.md"));
    for expected in [
        "research/methodology-notes.md",
        "research/tala-sa-pamamaraan.md",
        "research/review-reminders.md",
    ] {
        assert!(
            taglish
                .iter()
                .any(|result| result.document.relative_path == expected),
            "Taglish consent retrieval omitted {expected}"
        );
    }
}

#[test]
#[ignore = "requires verified local E5 and llama.cpp model files"]
fn r8_interpretation_deadline() {
    let prepared = prepared();
    let generation = GenerationGuard::new(&prepared.inputs);
    let cancel = AtomicBool::new(false);
    let benchmark_request = "Palitan sa project plan ang deadline na October 20 to October 23.";
    let workflow_request =
        "Hanapin yung project plan at palitan ang deadline na October 20 to October 23.";
    let benchmark_trace = interpretation::interpret_request_traced(
        generation.provider.as_ref(),
        benchmark_request,
        &prepared.documents,
        &prepared.contents,
        &prepared.chunks,
        &cancel,
    )
    .expect("real benchmark interpretation");
    let workflow_trace = interpretation::interpret_request_traced(
        generation.provider.as_ref(),
        workflow_request,
        &prepared.documents,
        &prepared.contents,
        &prepared.chunks,
        &cancel,
    )
    .expect("real workflow interpretation");
    write_evidence(
        "r8_interpretation_deadline",
        json!({
            "inputs": [benchmark_request, workflow_request],
            "results": {
                "benchmark": &benchmark_trace.result,
                "workflow": &workflow_trace.result,
            },
            "rawModelOutputs": {
                "benchmark": &benchmark_trace.raw_model_output,
                "workflow": &workflow_trace.raw_model_output,
            },
            "promptSha256": {
                "benchmark": &benchmark_trace.prompt_sha256,
                "workflow": &workflow_trace.prompt_sha256,
            },
        }),
    );
    let benchmark_result = benchmark_trace.result;
    let workflow_result = workflow_trace.result;
    assert_edit_proposal(&benchmark_result);
    assert_edit_proposal(&workflow_result);
}

#[test]
#[ignore = "requires verified local E5 and llama.cpp model files"]
fn r8_interpretation_ambiguity() {
    let prepared = prepared();
    let generation = GenerationGuard::new(&prepared.inputs);
    let cancel = AtomicBool::new(false);
    let request = "Rename the notes to archived-notes.md.";
    let trace = interpretation::interpret_request_traced(
        generation.provider.as_ref(),
        request,
        &prepared.documents,
        &prepared.contents,
        &prepared.chunks,
        &cancel,
    )
    .expect("real ambiguous interpretation");
    write_evidence(
        "r8_interpretation_ambiguity",
        json!({
            "input": request,
            "result": &trace.result,
            "rawModelOutput": &trace.raw_model_output,
            "promptSha256": &trace.prompt_sha256,
        }),
    );
    let result = trace.result;
    assert!(matches!(
        result,
        InterpretationResult::NeedsFileSelection { .. }
    ));
}

#[test]
#[ignore = "requires verified local E5 and llama.cpp model files"]
fn r8_evidence_gate() {
    let prepared = prepared();
    let queries = [
        (
            "englishToFilipino",
            "Community Learning Project submission deadline",
        ),
        (
            "filipinoToEnglish",
            "Hanapin ang plano at huling araw ng pagpasa ng proyekto",
        ),
        (
            "taglish",
            "Saan yung notes about consent ng interview participants?",
        ),
        (
            "unrelated",
            "What is the recipe for a chocolate cake and the bus schedule?",
        ),
    ];
    let mut query_evidence = serde_json::Map::new();
    let mut unrelated = Vec::new();
    for (label, query) in queries {
        let (results, scores) = retrieve(prepared, query, 8);
        if label == "unrelated" {
            unrelated = results.clone();
        }
        query_evidence.insert(
            label.into(),
            json!({
                "query": query,
                "rankedDocuments": retrieval_evidence(&results),
                "topKCosineScores": scores,
            }),
        );
    }
    let generation = GenerationGuard::new(&prepared.inputs);
    let cancel = AtomicBool::new(false);
    let counting = CountingProvider {
        inner: generation.provider.as_ref(),
        calls: AtomicUsize::new(0),
    };
    let unrelated_query = queries[3].1;
    let answer = grounding::answer_question(
        Some(&counting),
        unrelated_query,
        Vec::new(),
        Language::En,
        &cancel,
    )
    .expect("empty evidence returns an honest answer");
    let generator_calls = counting.calls.load(Ordering::Relaxed);
    write_evidence(
        "r8_evidence_gate",
        json!({
            "queries": query_evidence,
            "unrelatedResultCount": unrelated.len(),
            "answer": &answer,
            "generatorCalls": generator_calls,
        }),
    );
    assert!(
        unrelated.is_empty(),
        "unrelated query passed the evidence gate"
    );
    assert_eq!(&answer.kind, &GroundedAnswerKind::InsufficientEvidence);
    assert_eq!(generator_calls, 0);
}

#[test]
#[ignore = "requires verified local E5 and llama.cpp model files"]
fn r8_summary_cited_output() {
    let prepared = prepared();
    let generation = GenerationGuard::new(&prepared.inputs);
    let cancel = AtomicBool::new(false);
    let project_plan_chunks = prepared
        .chunks
        .iter()
        .filter(|chunk| chunk.document_id == "projects/project-plan.md")
        .cloned()
        .collect::<Vec<_>>();
    let supplied = grounding::passages_from_chunks(&project_plan_chunks);
    let summary = grounding::summarize_document(
        generation.provider.as_ref(),
        supplied.clone(),
        Language::Fil,
        &cancel,
    )
    .expect("real project-plan summary");
    let summary_text = summary.text.to_lowercase();
    let required_facts = [
        json!({
            "label": "October 20 deadline",
            "stringMatch": summary_text.contains("october 20"),
            "factualReview": "notReviewed",
        }),
        json!({
            "label": "12 volunteer students",
            "stringMatch": summary_text.contains("12 volunteer students"),
            "factualReview": "notReviewed",
        }),
        json!({
            "label": "October 24 presentation",
            "stringMatch": summary_text.contains("october 24"),
            "factualReview": "notReviewed",
        }),
    ];
    write_evidence(
        "r8_summary_cited_output",
        json!({
            "input": "projects/project-plan.md",
            "language": "fil",
            "result": &summary,
            "requiredFacts": &required_facts,
            "factualReview": "notReviewed",
        }),
    );
    assert!(
        !summary.sentences.is_empty(),
        "summary returned no sentences"
    );
    assert_citations_within(&summary, &supplied);
}

#[test]
#[ignore = "requires verified local E5 and llama.cpp model files"]
fn r8_cancellation_and_recovery() {
    let prepared = prepared();
    let generation = GenerationGuard::new(&prepared.inputs);
    let cancellation_schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "items": {
                "type": "array",
                "minItems": 128,
                "maxItems": 128,
                "items": { "type": "string" }
            }
        },
        "required": ["items"]
    });
    let cancellation_messages = vec![ChatMessage {
        role: "user".into(),
        content: format!(
            "Return exactly 128 short items in the JSON array. Work slowly and follow the schema. Context: {}",
            "This is neutral local test context. ".repeat(220)
        ),
    }];
    let cancellation_budget = GenerationBudget {
        max_output_tokens: 512,
        temperature: 0.0,
        seed: 7,
    };
    let cancellation_flag = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let cancellation_provider = Arc::clone(&generation.provider);
    let cancellation_flag_for_thread = Arc::clone(&cancellation_flag);
    let cancellation_started = Instant::now();
    let cancellation_thread = thread::spawn(move || {
        let result = cancellation_provider.generate_json(
            &cancellation_schema,
            &cancellation_messages,
            &cancellation_budget,
            cancellation_flag_for_thread.as_ref(),
        );
        sender.send(result).expect("cancellation result receiver");
    });
    thread::sleep(Duration::from_millis(250));
    cancellation_flag.store(true, Ordering::Release);
    generation
        .cancel_active()
        .expect("cancel active local generation");
    let cancellation_result = receiver.recv_timeout(Duration::from_secs(5));
    cancellation_thread
        .join()
        .expect("cancellation thread joins");
    let cancellation_elapsed = cancellation_started.elapsed();
    let follow_up = generation.generate_json(
        &json!({
            "type": "object",
            "additionalProperties": false,
            "properties": { "answer": { "type": "string" } },
            "required": ["answer"]
        }),
        &[ChatMessage {
            role: "user".into(),
            content: "Return JSON with answer equal to exactly ok.".into(),
        }],
        &GenerationBudget {
            max_output_tokens: 32,
            temperature: 0.0,
            seed: 7,
        },
        &AtomicBool::new(false),
    );
    let cancellation_status = match &cancellation_result {
        Ok(result) => result_evidence(result),
        Err(error) => json!({"error": error.to_string()}),
    };
    write_evidence(
        "r8_cancellation_and_recovery",
        json!({
            "cancellation": {
                "elapsedMs": cancellation_elapsed.as_millis(),
                "result": cancellation_status,
                "resultCode": cancellation_result
                    .as_ref()
                    .ok()
                    .and_then(|result| result.as_ref().err())
                    .and_then(provider_error_code)
                    .map(|code| format!("{code:?}")),
            },
            "followUp": result_evidence(&follow_up),
        }),
    );
    assert!(
        cancellation_result.is_ok(),
        "local generation cancellation did not finish within five seconds"
    );
    let cancellation_result = cancellation_result.expect("checked above");
    assert_eq!(
        cancellation_result
            .as_ref()
            .err()
            .and_then(provider_error_code),
        Some(ProviderErrorCode::Cancelled)
    );
    assert!(cancellation_elapsed < Duration::from_secs(5));
    let follow_up = follow_up.expect("next generation succeeds after cancellation");
    assert!(follow_up.get("answer").and_then(Value::as_str).is_some());
}
