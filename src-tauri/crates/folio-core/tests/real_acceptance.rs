use folio_core::chunking::{Chunk, ChunkSource, InterimTextChunker, TextDocument};
use folio_core::contracts::{
    DocumentRecord, GroundedAnswerKind, InterpretationResult, Language, ModelDescriptor, ModelFile,
    ModelRole, OperationProposal, ProviderErrorCode, SearchResult,
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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
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
const E5_REVISION: &str = "761b726dd34fb83930e26aab4e9ac3899aa1fa78";
const QWEN_MODEL_ID: &str = "qwen3-0.6b-q4-k-m";
const QWEN_REVISION: &str = "50968a4468ef4233ed78cd7c3de230dd1d61a56b";
const QWEN_MODEL_SHA256: &str = "ac2d97712095a558e31573f62f466a3f9d93990898b0ec79d7c974c1780d524a";
const QWEN_MODEL_BYTES: u64 = 396_705_472;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/documents")
}

fn fixture_corpus() -> (Vec<DocumentRecord>, HashMap<String, String>, Vec<Chunk>) {
    let mut sources = Vec::with_capacity(FIXTURE_PATHS.len());
    let mut contents = HashMap::with_capacity(FIXTURE_PATHS.len());
    for relative_path in FIXTURE_PATHS {
        let content = std::fs::read_to_string(fixture_root().join(relative_path))
            .unwrap_or_else(|error| panic!("read fixture {relative_path}: {error}"));
        let name = Path::new(relative_path)
            .file_name()
            .and_then(|value| value.to_str())
            .expect("fixture has a UTF-8 file name");
        let record = DocumentRecord {
            id: (*relative_path).into(),
            relative_path: (*relative_path).into(),
            name: name.into(),
            title: name.into(),
            language: grounding::detect_language(&content),
            size_bytes: content.len() as u64,
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

fn embedding_provider() -> OrtE5Provider {
    let model_path = std::env::var("FOLIO_E5_MODEL").expect("FOLIO_E5_MODEL");
    let tokenizer_path = std::env::var("FOLIO_E5_TOKENIZER").expect("FOLIO_E5_TOKENIZER");
    let model_sha256 = std::env::var("FOLIO_E5_MODEL_SHA256").expect("FOLIO_E5_MODEL_SHA256");
    let tokenizer_sha256 =
        std::env::var("FOLIO_E5_TOKENIZER_SHA256").expect("FOLIO_E5_TOKENIZER_SHA256");
    assert_eq!(model_sha256.len(), 64, "the E5 model hash must be supplied");
    assert_eq!(
        tokenizer_sha256.len(),
        64,
        "the E5 tokenizer hash must be supplied"
    );
    OrtE5Provider::from_files(
        model_path,
        tokenizer_path,
        E5_MODEL_ID,
        E5_REVISION,
        "int8",
        384,
        &model_sha256,
        &tokenizer_sha256,
        folio_core::embeddings::DEFAULT_MAX_TOKENS,
        folio_core::embeddings::DEFAULT_BATCH_SIZE,
        2,
    )
    .expect("local multilingual E5 provider loads")
}

fn generation_provider() -> LlamaServerProvider {
    let executable = std::env::var("FOLIO_LLAMA_SERVER").expect("FOLIO_LLAMA_SERVER");
    let model_path = std::env::var("FOLIO_QWEN_MODEL").expect("FOLIO_QWEN_MODEL");
    let model_path = PathBuf::from(model_path);
    let model_hash = std::env::var("FOLIO_QWEN_MODEL_SHA256").expect("FOLIO_QWEN_MODEL_SHA256");
    let model_bytes = std::fs::metadata(&model_path)
        .expect("Qwen model metadata")
        .len();
    assert_eq!(model_hash, QWEN_MODEL_SHA256);
    assert_eq!(model_bytes, QWEN_MODEL_BYTES);
    let descriptor = ModelDescriptor {
        id: QWEN_MODEL_ID.into(),
        role: ModelRole::Generation,
        repo: "unsloth/Qwen3-0.6B-GGUF".into(),
        revision: QWEN_REVISION.into(),
        files: vec![ModelFile {
            path: model_path
                .file_name()
                .and_then(|value| value.to_str())
                .expect("Qwen model has a UTF-8 file name")
                .into(),
            sha256: model_hash,
            bytes: model_bytes,
            download_url: None,
        }],
        quantization: "Q4_K_M".into(),
        license: "apache-2.0".into(),
        runtime: "llama.cpp".into(),
        optional_pack: false,
    };
    LlamaServerProvider::from_verified_model(
        executable,
        folio_core::models::VerifiedModelFile {
            descriptor,
            path: model_path,
        },
        2,
    )
    .expect("local llama provider loads")
}

fn search_fixture(
    provider: &OrtE5Provider,
    retriever: &HybridRetriever,
    documents: &[DocumentRecord],
    chunks: &[Chunk],
    query: &str,
    limit: usize,
) -> Vec<SearchResult> {
    let query_embedding = provider
        .embed_query(query, None)
        .unwrap_or_else(|error| panic!("embed query {query:?}: {error}"));
    retriever
        .search(documents, chunks, query, Some(&query_embedding), limit)
        .unwrap_or_else(|error| panic!("retrieve query {query:?}: {error}"))
}

fn print_retrieval(label: &str, query: &str, results: &[SearchResult]) {
    let rendered = results
        .iter()
        .map(|result| {
            json!({
                "path": result.document.relative_path,
                "score": result.score,
                "method": result.method,
                "passages": result.passages.iter().map(|passage| &passage.text).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    println!(
        "R8 retrieval {label} query={query:?}: {}",
        serde_json::to_string(&rendered).expect("retrieval output serializes")
    );
}

fn assert_edit_proposal(result: &InterpretationResult, request: &str) {
    println!(
        "R8 interpretation request={request:?}: {}",
        serde_json::to_string(result).expect("interpretation output serializes")
    );
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

#[test]
#[ignore = "requires verified local E5 and llama.cpp model files"]
fn real_folio_acceptance_cases_use_local_providers() {
    let (documents, contents, chunks) = fixture_corpus();
    let embeddings = embedding_provider();
    let passage_texts = chunks
        .iter()
        .map(|chunk| chunk.text.clone())
        .collect::<Vec<_>>();
    let vectors = embeddings
        .embed(&passage_texts, EmbeddingKind::Passage, None)
        .expect("real E5 passage embeddings");
    let mut retriever = HybridRetriever::default();
    retriever
        .vector_index
        .replace(embeddings.space().clone(), chunks.clone(), vectors)
        .expect("fixture vectors fit the E5 space");

    let english = search_fixture(
        &embeddings,
        &retriever,
        &documents,
        &chunks,
        "Community Learning Project submission deadline",
        5,
    );
    print_retrieval(
        "EN-to-FIL",
        "Community Learning Project submission deadline",
        &english,
    );
    assert!(english
        .iter()
        .any(|result| result.document.relative_path == "notes/tala-sa-proyekto.md"));
    assert!(!english
        .iter()
        .any(|result| result.document.relative_path == "personal/grocery-list.md"));

    let filipino = search_fixture(
        &embeddings,
        &retriever,
        &documents,
        &chunks,
        "Hanapin ang plano at huling araw ng pagpasa ng proyekto",
        5,
    );
    print_retrieval(
        "FIL-to-EN",
        "Hanapin ang plano at huling araw ng pagpasa ng proyekto",
        &filipino,
    );
    assert!(filipino
        .iter()
        .any(|result| result.document.relative_path == "projects/project-plan.md"));

    let taglish = search_fixture(
        &embeddings,
        &retriever,
        &documents,
        &chunks,
        "Saan yung notes about consent ng interview participants?",
        5,
    );
    print_retrieval(
        "Taglish-consent",
        "Saan yung notes about consent ng interview participants?",
        &taglish,
    );
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

    let generation = Arc::new(generation_provider());
    let cancel = AtomicBool::new(false);
    let benchmark_request = "Palitan sa project plan ang deadline na October 20 to October 23.";
    let benchmark_result = interpretation::interpret_request(
        generation.as_ref(),
        benchmark_request,
        &documents,
        &contents,
        &chunks,
        &cancel,
    )
    .expect("real benchmark interpretation");
    assert_edit_proposal(&benchmark_result, benchmark_request);

    let workflow_request =
        "Hanapin yung project plan at palitan ang deadline na October 20 to October 23.";
    let workflow_result = interpretation::interpret_request(
        generation.as_ref(),
        workflow_request,
        &documents,
        &contents,
        &chunks,
        &cancel,
    )
    .expect("real workflow interpretation");
    assert_edit_proposal(&workflow_result, workflow_request);

    let ambiguous_request = "Rename the notes to archived-notes.md.";
    let ambiguous_result = interpretation::interpret_request(
        generation.as_ref(),
        ambiguous_request,
        &documents,
        &contents,
        &chunks,
        &cancel,
    )
    .expect("real ambiguous interpretation");
    println!(
        "R8 interpretation request={ambiguous_request:?}: {}",
        serde_json::to_string(&ambiguous_result).expect("ambiguous output serializes")
    );
    assert!(matches!(
        ambiguous_result,
        InterpretationResult::NeedsFileSelection { .. }
    ));

    let unrelated_query = "What is the recipe for a chocolate cake and the bus schedule?";
    let unrelated = search_fixture(
        &embeddings,
        &retriever,
        &documents,
        &chunks,
        unrelated_query,
        8,
    );
    print_retrieval("unrelated", unrelated_query, &unrelated);
    assert!(
        unrelated.is_empty(),
        "unrelated query passed the evidence gate"
    );
    let counting = CountingProvider {
        inner: generation.as_ref(),
        calls: AtomicUsize::new(0),
    };
    let answer = grounding::answer_question(
        Some(&counting),
        unrelated_query,
        Vec::new(),
        Language::En,
        &cancel,
    )
    .expect("empty evidence returns an honest answer");
    assert_eq!(answer.kind, GroundedAnswerKind::InsufficientEvidence);
    assert_eq!(counting.calls.load(Ordering::Relaxed), 0);
    println!(
        "R8 unrelated answer: {}",
        serde_json::to_string(&answer).expect("unrelated answer serializes")
    );

    let project_plan_chunks = chunks
        .iter()
        .filter(|chunk| chunk.document_id == "projects/project-plan.md")
        .cloned()
        .collect::<Vec<_>>();
    let summary = grounding::summarize_document(
        generation.as_ref(),
        grounding::passages_from_chunks(&project_plan_chunks),
        Language::Fil,
        &cancel,
    )
    .expect("real project-plan summary");
    assert!(
        !summary.sentences.is_empty(),
        "summary returned no sentences"
    );
    assert!(summary
        .sentences
        .iter()
        .all(|sentence| !sentence.citations.is_empty()));
    let summary_text = summary.text.to_lowercase();
    let required_fact_checks = [
        ("October 20 deadline", summary_text.contains("october 20")),
        (
            "12 volunteer students",
            summary_text.contains("12 volunteer students"),
        ),
        (
            "October 24 presentation",
            summary_text.contains("october 24"),
        ),
    ];
    println!(
        "R8 summary required-fact checks (TJ factual review required): {}",
        serde_json::to_string(&required_fact_checks).expect("fact checks serialize")
    );
    println!(
        "R8 summary output (TJ factual review required): {}",
        serde_json::to_string(&summary).expect("summary serializes")
    );

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
    let cancellation_provider = Arc::clone(&generation);
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
    let cancellation_result = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("local generation cancellation completed promptly");
    cancellation_thread
        .join()
        .expect("cancellation thread joins");
    let cancellation_elapsed = cancellation_started.elapsed();
    println!(
        "R8 cancellation elapsed_ms={} result_code={:?}",
        cancellation_elapsed.as_millis(),
        cancellation_result
            .as_ref()
            .err()
            .and_then(provider_error_code)
    );
    assert_eq!(
        cancellation_result
            .as_ref()
            .err()
            .and_then(provider_error_code),
        Some(ProviderErrorCode::Cancelled)
    );
    assert!(cancellation_elapsed < Duration::from_secs(5));

    let follow_up = generation
        .generate_json(
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
        )
        .expect("next generation succeeds after cancellation");
    println!(
        "R8 post-cancellation generation: {}",
        serde_json::to_string(&follow_up).expect("follow-up output serializes")
    );
    assert!(follow_up.get("answer").and_then(Value::as_str).is_some());

    generation
        .unload()
        .expect("unload local generation provider");
}
