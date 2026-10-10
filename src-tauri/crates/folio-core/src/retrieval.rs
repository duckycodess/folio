use crate::chunking::Chunk;
use crate::contracts::ProviderErrorCode;
use crate::contracts::{DocumentRecord, EmbeddingSpace, SearchMethod, SearchResult, SourcePassage};
use crate::embeddings::QueryEmbedding;
use crate::error::{CoreError, CoreResult, NativeProviderErrorError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

const DEFAULT_PASSAGES_PER_DOCUMENT: usize = 3;

/// Provisional cosine gate calibrated only against the development corpus.
/// Revisit it when #3 supplies persisted retrieval evaluation data.
pub const MIN_SEMANTIC_SCORE: f32 = 0.35;
/// Keyword evidence may supplement semantic hits only when a chunk carries at
/// least half of the query's IDF-weighted BM25 mass. This remains an interim
/// fallback floor until #3 supplies FTS5.
pub const MIN_KEYWORD_SCORE: f32 = 0.5;
/// Keyword evidence only reorders semantic near-ties. Multilingual E5 cosine
/// scores for one query usually differ by a few hundredths, so a bounded
/// weight keeps lexical overlap from outranking stronger semantic evidence
/// (for example a distractor that repeats the query words in a negated
/// sentence). This is a design bound, not a value fitted to any query.
pub const KEYWORD_TIEBREAK_WEIGHT: f32 = 0.01;
/// Query-level evidence gate. Multilingual E5 cosines for unrelated text sit
/// in the same narrow high band as related text, so a per-chunk cosine floor
/// cannot say "no evidence" on its own. The gate therefore also requires the
/// best chunk to stand out from the median of the whole indexed space.
///
/// PROVISIONAL (Q5-Q7 style). Set from the development calibration queries in
/// `tests/dev_calibration.json` only, never from the acceptance cases, using
/// R8 run 37949760186 (E5 int8 `761b726d`, title/path passage context). Rule:
/// the lowest three-decimal value in the interval that maximizes related
/// passes minus unrelated passes. On that data the margins overlap (related
/// minimum 0.0262, unrelated maximum 0.0304): the gate passes 7 of 8 related
/// and 0 of 6 unrelated development queries. Recalibrate when the embedding
/// space, chunking or corpus changes.
pub const GATE_MIN_TOP_COSINE: f32 = 0.813;
pub const GATE_MIN_MARGIN: f32 = 0.031;
pub const GATE_MIN_CHUNKS_FOR_MARGIN: usize = 5;
const BM25_K1: f32 = 1.2;
const BM25_B: f32 = 0.75;

#[derive(Clone, Debug)]
struct IndexedSpace {
    space: EmbeddingSpace,
    chunks: Vec<Chunk>,
    vectors: Vec<Vec<f32>>,
}

#[derive(Clone, Debug, Default)]
pub struct VectorIndex {
    spaces: HashMap<String, IndexedSpace>,
}

impl VectorIndex {
    pub fn replace(
        &mut self,
        space: EmbeddingSpace,
        chunks: Vec<Chunk>,
        vectors: Vec<Vec<f32>>,
    ) -> CoreResult<String> {
        if chunks.len() != vectors.len() {
            return Err(CoreError::Message(
                "Each indexed chunk must have exactly one vector.".into(),
            ));
        }
        if vectors
            .iter()
            .any(|vector| vector.len() != space.dimensions)
        {
            return Err(CoreError::Message(
                "A vector dimension does not match its embedding space.".into(),
            ));
        }
        let id = embedding_space_id(&space);
        self.spaces.insert(
            id.clone(),
            IndexedSpace {
                space,
                chunks,
                vectors,
            },
        );
        Ok(id)
    }

    pub fn clear_except(&mut self, space: &EmbeddingSpace) {
        let id = embedding_space_id(space);
        self.spaces.retain(|key, _| key == &id);
    }

    pub fn len_for(&self, space: &EmbeddingSpace) -> usize {
        self.spaces
            .get(&embedding_space_id(space))
            .map_or(0, |indexed| indexed.chunks.len())
    }

    /// The chunks and vectors indexed for exactly this space, if any. Vectors
    /// of other spaces are never returned with them.
    pub fn indexed(&self, space: &EmbeddingSpace) -> Option<(&[Chunk], &[Vec<f32>])> {
        self.spaces
            .get(&embedding_space_id(space))
            .filter(|indexed| &indexed.space == space)
            .map(|indexed| (indexed.chunks.as_slice(), indexed.vectors.as_slice()))
    }

    pub fn search(&self, query: &QueryEmbedding, limit: usize) -> CoreResult<Vec<(Chunk, f32)>> {
        self.search_scoped(query, None, limit)
    }

    pub fn search_scoped(
        &self,
        query: &QueryEmbedding,
        document_id: Option<&str>,
        limit: usize,
    ) -> CoreResult<Vec<(Chunk, f32)>> {
        if query.vector.len() != query.space.dimensions {
            return Err(CoreError::Message(
                "Query vector dimension does not match its embedding space.".into(),
            ));
        }
        let requested_id = embedding_space_id(&query.space);
        let indexed = self.spaces.get(&requested_id).ok_or_else(|| {
            CoreError::Provider(NativeProviderErrorError::new(
                ProviderErrorCode::EmbeddingSpaceMismatch,
                "The query embedding space is not indexed.",
            ))
        })?;
        if indexed.space != query.space {
            return Err(CoreError::Provider(NativeProviderErrorError::new(
                ProviderErrorCode::EmbeddingSpaceMismatch,
                "The query embedding space does not match the index.",
            )));
        }
        let mut scored = indexed
            .chunks
            .iter()
            .cloned()
            .zip(indexed.vectors.iter())
            .filter(|(chunk, _)| document_id.is_none_or(|id| chunk.document_id == id))
            .map(|(chunk, vector)| (chunk, cosine_similarity(&query.vector, vector)))
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(limit);
        Ok(scored)
    }
}

#[derive(Clone)]
pub struct HybridRetriever {
    pub vector_index: VectorIndex,
    pub max_passages: usize,
}

impl Default for HybridRetriever {
    fn default() -> Self {
        Self {
            vector_index: VectorIndex::default(),
            max_passages: DEFAULT_PASSAGES_PER_DOCUMENT,
        }
    }
}

/// Per-chunk ranking evidence for one query. Diagnostic only: it lets an
/// acceptance record show why a document did or did not rank.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkScore {
    pub document_id: String,
    pub ordinal: usize,
    pub cosine: f32,
    pub keyword: f32,
    pub fused: f32,
}

/// Query-level semantic evidence statistics over the whole indexed space.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceGate {
    pub top_cosine: f32,
    pub median_cosine: f32,
    pub margin: f32,
    pub chunk_count: usize,
    pub min_top_cosine: f32,
    pub min_margin: f32,
    pub passed: bool,
}

impl EvidenceGate {
    /// The gate over every cosine in scope, not only the top candidates: the
    /// margin is measured against the median of the whole space.
    pub fn from_cosines(mut cosines: Vec<f32>) -> Self {
        cosines.sort_by(|a, b| b.total_cmp(a));
        let chunk_count = cosines.len();
        let top_cosine = cosines.first().copied().unwrap_or(0.0);
        let median_cosine = if chunk_count == 0 {
            0.0
        } else if chunk_count % 2 == 1 {
            cosines[chunk_count / 2]
        } else {
            (cosines[chunk_count / 2 - 1] + cosines[chunk_count / 2]) / 2.0
        };
        let margin = top_cosine - median_cosine;
        let margin_applies = chunk_count >= GATE_MIN_CHUNKS_FOR_MARGIN;
        let passed = chunk_count > 0
            && top_cosine >= GATE_MIN_TOP_COSINE
            && (!margin_applies || margin >= GATE_MIN_MARGIN);
        Self {
            top_cosine,
            median_cosine,
            margin,
            chunk_count,
            min_top_cosine: GATE_MIN_TOP_COSINE,
            min_margin: GATE_MIN_MARGIN,
            passed,
        }
    }
}

/// Whether a chunk is a candidate: semantic evidence needs the query to pass
/// the gate and the chunk to clear the semantic floor; strong keyword
/// evidence is enough on its own.
pub fn admits(gate: &EvidenceGate, cosine: f32, keyword: f32) -> bool {
    (gate.passed && cosine >= MIN_SEMANTIC_SCORE) || keyword >= MIN_KEYWORD_SCORE
}

/// Cosine plus the bounded keyword tiebreak.
pub fn fused(cosine: f32, keyword: f32) -> f32 {
    cosine + KEYWORD_TIEBREAK_WEIGHT * keyword
}

impl HybridRetriever {
    /// Whether the query has semantic evidence anywhere in the indexed space.
    pub fn evidence_gate(&self, query_embedding: &QueryEmbedding) -> CoreResult<EvidenceGate> {
        self.evidence_gate_scoped(query_embedding, None)
    }

    /// The evidence gate over one document's chunks when the search is scoped
    /// to it, so another document cannot open the gate for that search.
    pub fn evidence_gate_scoped(
        &self,
        query_embedding: &QueryEmbedding,
        document_id: Option<&str>,
    ) -> CoreResult<EvidenceGate> {
        let cosines = self
            .vector_index
            .search_scoped(query_embedding, document_id, usize::MAX)?
            .into_iter()
            .map(|(_, cosine)| cosine)
            .collect();
        Ok(EvidenceGate::from_cosines(cosines))
    }

    /// Keyword-only retrieval with BM25 scores normalized to the query's
    /// IDF-weighted maximum. Labelled `keyword`, never `semantic`. Chunks below
    /// `MIN_KEYWORD_SCORE` are not evidence, so a query made only of common
    /// words retrieves nothing and no generation runs.
    pub fn keyword(
        &self,
        documents: &[DocumentRecord],
        chunks: &[Chunk],
        query: &str,
        limit: usize,
    ) -> Vec<SearchResult> {
        self.keyword_scoped(documents, chunks, query, None, limit)
    }

    fn keyword_scoped(
        &self,
        documents: &[DocumentRecord],
        chunks: &[Chunk],
        query: &str,
        document_id: Option<&str>,
        limit: usize,
    ) -> Vec<SearchResult> {
        let terms = query_terms(query);
        if terms.is_empty() {
            return Vec::new();
        }
        let scores = bm25_scores(chunks, &terms);
        let scored = chunks
            .iter()
            .zip(scores)
            .filter(|(chunk, score)| {
                *score >= MIN_KEYWORD_SCORE && document_id.is_none_or(|id| chunk.document_id == id)
            })
            .map(|(chunk, score)| (chunk, score, score))
            .collect();
        self.to_results(
            documents_by_id(documents),
            scored,
            SearchMethod::Keyword,
            None,
            limit,
        )
    }

    /// The fraction of distinct query terms that occur in a chunk. Kept for
    /// interpretation target resolution, whose ambiguity margins are defined
    /// on this scale; search ranking uses BM25 instead.
    pub fn keyword_term_overlap(
        &self,
        documents: &[DocumentRecord],
        chunks: &[Chunk],
        query: &str,
        limit: usize,
    ) -> Vec<SearchResult> {
        let terms = query_terms(query);
        if terms.is_empty() {
            return Vec::new();
        }
        let mut scored = Vec::new();
        for chunk in chunks {
            let searchable = chunk.text.to_lowercase();
            let matched = terms
                .iter()
                .filter(|term| searchable.contains(term.as_str()))
                .count();
            if matched == 0 {
                continue;
            }
            let score = matched as f32 / terms.len() as f32;
            scored.push((chunk, score, score));
        }
        self.to_results(
            documents_by_id(documents),
            scored,
            SearchMethod::Keyword,
            None,
            limit,
        )
    }

    pub fn search(
        &self,
        documents: &[DocumentRecord],
        chunks: &[Chunk],
        query: &str,
        semantic: Option<&QueryEmbedding>,
        limit: usize,
    ) -> CoreResult<Vec<SearchResult>> {
        self.search_scoped(documents, chunks, query, semantic, None, limit)
    }

    /// Semantic-primary hybrid retrieval. Every chunk in scope is scored by
    /// cosine; BM25 keyword evidence adds at most `KEYWORD_TIEBREAK_WEIGHT`.
    /// A chunk is a candidate when the query passes the evidence gate and the
    /// chunk passes the semantic floor, or when it carries strong keyword
    /// evidence on its own.
    pub fn search_scoped(
        &self,
        documents: &[DocumentRecord],
        chunks: &[Chunk],
        query: &str,
        semantic: Option<&QueryEmbedding>,
        document_id: Option<&str>,
        limit: usize,
    ) -> CoreResult<Vec<SearchResult>> {
        let Some(query_embedding) = semantic else {
            return Ok(self.keyword_scoped(documents, chunks, query, document_id, limit));
        };
        let gate = self.evidence_gate_scoped(query_embedding, document_id)?;
        let scores = self.score_chunks(chunks, query, query_embedding, document_id)?;
        let combined = scores
            .iter()
            .filter(|(_, score)| admits(&gate, score.cosine, score.keyword))
            .map(|(chunk, score)| (chunk, score.cosine, score.fused))
            .collect::<Vec<_>>();
        Ok(self.to_results(
            documents_by_id(documents),
            combined,
            SearchMethod::Hybrid,
            Some(space_fingerprint(&query_embedding.space)),
            limit,
        ))
    }

    /// Cosine, keyword and fused scores for every chunk in scope, highest
    /// fused score first.
    pub fn explain(
        &self,
        chunks: &[Chunk],
        query: &str,
        query_embedding: &QueryEmbedding,
    ) -> CoreResult<Vec<ChunkScore>> {
        Ok(self
            .score_chunks(chunks, query, query_embedding, None)?
            .into_iter()
            .map(|(_, score)| score)
            .collect())
    }

    fn score_chunks(
        &self,
        chunks: &[Chunk],
        query: &str,
        query_embedding: &QueryEmbedding,
        document_id: Option<&str>,
    ) -> CoreResult<Vec<(Chunk, ChunkScore)>> {
        let terms = query_terms(query);
        let keyword_by_key = chunks
            .iter()
            .zip(bm25_scores(chunks, &terms))
            .map(|(chunk, score)| ((chunk.document_id.clone(), chunk.ordinal), score))
            .collect::<HashMap<_, _>>();
        let semantic =
            self.vector_index
                .search_scoped(query_embedding, document_id, chunks.len())?;
        let mut scored = semantic
            .into_iter()
            .map(|(chunk, cosine)| {
                let keyword = keyword_by_key
                    .get(&(chunk.document_id.clone(), chunk.ordinal))
                    .copied()
                    .unwrap_or(0.0);
                let score = ChunkScore {
                    document_id: chunk.document_id.clone(),
                    ordinal: chunk.ordinal,
                    cosine,
                    keyword,
                    fused: fused(cosine, keyword),
                };
                (chunk, score)
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.1.fused.total_cmp(&a.1.fused));
        Ok(scored)
    }

    /// Group scored chunks into documents; see [`group_passages`].
    fn to_results<'a>(
        &self,
        by_id: HashMap<&'a str, &'a DocumentRecord>,
        scored: Vec<(&'a Chunk, f32, f32)>,
        method: SearchMethod,
        space_fingerprint: Option<String>,
        limit: usize,
    ) -> Vec<SearchResult> {
        let passages = scored
            .into_iter()
            .map(|(chunk, _raw_score, result_score)| {
                (
                    SourcePassage {
                        document_id: chunk.document_id.clone(),
                        document_content_hash: chunk.content_hash.clone(),
                        offset_unit: crate::contracts::OffsetUnit::Utf8Byte,
                        start: chunk.start,
                        end: chunk.end,
                        text: chunk.text.clone(),
                        page: None,
                    },
                    result_score,
                )
            })
            .collect();
        group_passages(
            by_id,
            passages,
            method,
            space_fingerprint,
            self.max_passages,
            limit,
        )
    }
}

/// Group scored passages into documents, best document first. `limit` counts
/// distinct document contents: a byte-identical copy is listed next to its
/// original without using another result slot, so duplicates cannot crowd out
/// other evidence. Exact-duplicate reporting itself stays with Organize.
/// Passages whose document is not in `by_id` are dropped.
pub fn group_passages(
    by_id: HashMap<&str, &DocumentRecord>,
    mut scored: Vec<(SourcePassage, f32)>,
    method: SearchMethod,
    space_fingerprint: Option<String>,
    max_passages: usize,
    limit: usize,
) -> Vec<SearchResult> {
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut grouped: HashMap<String, (f32, Vec<SourcePassage>)> = HashMap::new();
    for (passage, result_score) in scored {
        let entry = grouped
            .entry(passage.document_id.clone())
            .or_insert_with(|| (result_score, Vec::new()));
        entry.0 = entry.0.max(result_score);
        if entry.1.len() < max_passages {
            entry.1.push(passage);
        }
    }
    let mut results = grouped
        .into_iter()
        .filter_map(|(document_id, (score, passages))| {
            by_id
                .get(document_id.as_str())
                .map(|document| SearchResult {
                    document: (*document).clone(),
                    passages,
                    score,
                    method: method.clone(),
                    space_fingerprint: space_fingerprint.clone(),
                })
        })
        .collect::<Vec<_>>();
    results.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.document.relative_path.cmp(&b.document.relative_path))
    });
    limit_distinct_contents(results, limit)
}

fn documents_by_id(documents: &[DocumentRecord]) -> HashMap<&str, &DocumentRecord> {
    documents
        .iter()
        .map(|document| (document.id.as_str(), document))
        .collect()
}

fn content_identity(result: &SearchResult) -> Option<String> {
    result.document.content_hash.clone().or_else(|| {
        result
            .passages
            .first()
            .map(|passage| passage.document_content_hash.clone())
    })
}

fn limit_distinct_contents(results: Vec<SearchResult>, limit: usize) -> Vec<SearchResult> {
    let mut seen = HashSet::new();
    let mut distinct = 0_usize;
    let mut kept = Vec::new();
    for result in results {
        let identity = content_identity(&result);
        let duplicate = identity.as_ref().is_some_and(|hash| seen.contains(hash));
        if !duplicate {
            if distinct >= limit {
                continue;
            }
            distinct += 1;
            if let Some(hash) = identity {
                seen.insert(hash);
            }
        }
        kept.push(result);
    }
    kept
}

pub fn tokens(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The query's distinct lowercase terms, sorted.
pub fn query_terms(query: &str) -> Vec<String> {
    let mut terms = tokens(query)
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    terms.sort();
    terms
}

/// English and Filipino function words that never make a chunk evidence.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is", "it", "of", "on",
    "or", "that", "the", "this", "to", "was", "with", "ang", "ay", "ito", "kay", "mga", "na",
    "nang", "ng", "ni", "pa", "para", "po", "sa", "si", "yung",
];

fn is_stop_word(term: &str) -> bool {
    STOP_WORDS.contains(&term)
}

/// Corpus statistics BM25 needs: how many chunks are in scope, their average
/// length in tokens, and how many contain each query term (aligned with the
/// terms).
#[derive(Clone, Debug)]
pub struct Bm25Stats {
    pub chunk_count: usize,
    pub average_length: f32,
    pub document_frequency: Vec<usize>,
}

/// BM25 normalized by the summed IDF weights of the query's informative terms
/// (the score of a chunk of average length containing each of them once). Stop
/// words and terms found in nearly every chunk carry no weight, so a query of
/// only common words has no keyword evidence. Scores are in [0, 1].
pub struct Bm25Scorer<'a> {
    terms: &'a [String],
    weights: Vec<f32>,
    maximum: f32,
    average_length: f32,
}

impl<'a> Bm25Scorer<'a> {
    pub fn new(terms: &'a [String], stats: &Bm25Stats) -> Self {
        let count = stats.chunk_count as f32;
        let weights = terms
            .iter()
            .zip(&stats.document_frequency)
            .map(|(term, frequency)| {
                let frequency = *frequency as f32;
                // Function words ("the", "ang") and a term in nearly every chunk (a
                // shared header word) say nothing about which chunk is evidence. A
                // meaningful word that is merely common, such as a project's name,
                // keeps its (low) weight.
                if is_stop_word(term) || frequency >= (count * 0.9).max(2.0) {
                    return 0.0;
                }
                (1.0 + (count - frequency + 0.5) / (frequency + 0.5)).ln()
            })
            .collect::<Vec<_>>();
        let maximum = weights.iter().sum::<f32>();
        Self {
            terms,
            weights,
            maximum,
            average_length: stats.average_length.max(1.0),
        }
    }

    pub fn score(&self, chunk_tokens: &[String]) -> f32 {
        let length_norm =
            BM25_K1 * (1.0 - BM25_B + BM25_B * chunk_tokens.len() as f32 / self.average_length);
        let score = self
            .terms
            .iter()
            .zip(&self.weights)
            .map(|(term, weight)| {
                let frequency = chunk_tokens.iter().filter(|token| *token == term).count() as f32;
                if frequency == 0.0 {
                    0.0
                } else {
                    weight * frequency * (BM25_K1 + 1.0) / (frequency + length_norm)
                }
            })
            .sum::<f32>();
        if self.maximum > 0.0 {
            (score / self.maximum).min(1.0)
        } else {
            0.0
        }
    }
}

/// BM25 over the supplied chunks. Returns one score in [0, 1] per chunk,
/// aligned with `chunks`.
fn bm25_scores(chunks: &[Chunk], terms: &[String]) -> Vec<f32> {
    if chunks.is_empty() || terms.is_empty() {
        return vec![0.0; chunks.len()];
    }
    let documents = chunks
        .iter()
        .map(|chunk| tokens(&chunk.text))
        .collect::<Vec<_>>();
    let stats = Bm25Stats {
        chunk_count: documents.len(),
        average_length: documents.iter().map(Vec::len).sum::<usize>() as f32
            / documents.len() as f32,
        document_frequency: terms
            .iter()
            .map(|term| {
                documents
                    .iter()
                    .filter(|tokens| tokens.iter().any(|token| token == term))
                    .count()
            })
            .collect(),
    };
    let scorer = Bm25Scorer::new(terms, &stats);
    documents
        .iter()
        .map(|tokens| scorer.score(tokens))
        .collect()
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[derive(Serialize)]
struct SpaceFingerprint<'a> {
    model_id: &'a str,
    revision: &'a str,
    quantization: &'a str,
    dimensions: usize,
    preprocessing_fingerprint: &'a str,
}

fn embedding_space_id(space: &EmbeddingSpace) -> String {
    let fingerprint = SpaceFingerprint {
        model_id: &space.model_id,
        revision: &space.revision,
        quantization: &space.quantization,
        dimensions: space.dimensions,
        preprocessing_fingerprint: &space.preprocessing_fingerprint,
    };
    hex::encode(Sha256::digest(
        serde_json::to_vec(&fingerprint).expect("space fingerprint is serializable"),
    ))
}

fn escape_fingerprint_field(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}

pub fn space_fingerprint(space: &EmbeddingSpace) -> String {
    format!(
        "folio-space-v1/{}/{}/{}/{}/{}",
        escape_fingerprint_field(&space.model_id),
        escape_fingerprint_field(&space.revision),
        escape_fingerprint_field(&space.quantization),
        space.dimensions,
        escape_fingerprint_field(&space.preprocessing_fingerprint),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunking::{content_hash, ChunkSource, InterimTextChunker, TextDocument};
    use crate::contracts::{DocumentRecord, Language};

    fn document(id: &str, content: &str) -> TextDocument {
        TextDocument::new(
            DocumentRecord {
                id: id.into(),
                workspace_id: "test-workspace".into(),
                relative_path: id.into(),
                name: id.rsplit('/').next().unwrap_or(id).into(),
                title: id.into(),
                language: Language::Mixed,
                media_type: "text/markdown".into(),
                size_bytes: content.len() as u64,
                modified_at_ms: None,
                content: None,
                content_hash: None,
            },
            content,
        )
    }

    fn space(revision: &str) -> EmbeddingSpace {
        EmbeddingSpace {
            model_id: "e5".into(),
            revision: revision.into(),
            quantization: "int8".into(),
            dimensions: 2,
            preprocessing_fingerprint: "chunk-v1".into(),
        }
    }

    #[test]
    fn keyword_fallback_is_labelled_and_returns_passages() {
        let source = InterimTextChunker::new(vec![document(
            "notes.md",
            "Ang pagpupulong ay sa Biyernes.",
        )]);
        let documents = source.documents();
        let chunks = source.all_chunks().unwrap();
        let retriever = HybridRetriever::default();
        let results = retriever
            .search(&documents, &chunks, "pagpupulong Biyernes", None, 5)
            .unwrap();
        assert_eq!(results[0].method, SearchMethod::Keyword);
        assert!(results[0].passages[0].text.contains("Biyernes"));
    }

    #[test]
    fn bm25_gives_common_words_little_weight() {
        let source = InterimTextChunker::new(vec![
            document("a.md", "the plan is in the folder and the notes"),
            document("b.md", "the budget is for the bus and the cake"),
            document("c.md", "the schedule is the same as the plan"),
        ]);
        let documents = source.documents();
        let chunks = source.all_chunks().unwrap();
        let retriever = HybridRetriever::default();
        // Only common words match a.md; the rare words appear nowhere.
        let results = retriever.keyword(&documents, &chunks, "what is the chocolate recipe", 5);
        assert!(results
            .iter()
            .all(|result| result.score < MIN_KEYWORD_SCORE));
        // A rare word carries most of the weight.
        let results = retriever.keyword(&documents, &chunks, "the budget", 5);
        assert_eq!(results[0].document.id, "b.md");
        assert!(results[0].score >= MIN_KEYWORD_SCORE);
    }

    #[test]
    fn keyword_only_mode_applies_the_evidence_floor() {
        let source = InterimTextChunker::new(vec![
            document("a.md", "ang plano ay nasa folder at ang tala"),
            document("b.md", "ang budget ay para sa bus at ang cake"),
        ]);
        let documents = source.documents();
        let chunks = source.all_chunks().unwrap();
        let retriever = HybridRetriever::default();
        assert!(retriever
            .search(&documents, &chunks, "ang at", None, 5)
            .unwrap()
            .is_empty());
        let results = retriever
            .search(&documents, &chunks, "budget", None, 5)
            .unwrap();
        assert_eq!(results[0].document.id, "b.md");
    }

    #[test]
    fn a_meaningful_word_in_most_files_still_matches_in_keyword_mode() {
        // "project" is in 3 of 5 files, like 9 of the 15 sample files; it is
        // common, not meaningless, so keyword-only search still finds it.
        let source = InterimTextChunker::new(vec![
            document("plan.md", "project plan due October 20"),
            document("notes.md", "meeting notes for the project"),
            document("checklist.md", "project submission checklist"),
            document("budget.md", "monthly budget for transport"),
            document("grocery.md", "grocery list for the week"),
        ]);
        let documents = source.documents();
        let chunks = source.all_chunks().unwrap();
        let retriever = HybridRetriever::default();
        let results = retriever
            .search(&documents, &chunks, "project", None, 5)
            .unwrap();
        let mut found = results
            .iter()
            .map(|result| result.document.id.as_str())
            .collect::<Vec<_>>();
        found.sort();
        assert_eq!(found, ["checklist.md", "notes.md", "plan.md"]);
        // Function words alone are still not evidence.
        assert!(retriever
            .search(&documents, &chunks, "the for", None, 5)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_scoped_search_uses_the_gate_of_its_own_document() {
        let source = InterimTextChunker::new(vec![
            document("strong.md", "matching evidence"),
            document("target.md", "unrelated text"),
        ]);
        let documents = source.documents();
        let chunks = source.all_chunks().unwrap();
        let mut retriever = HybridRetriever::default();
        let space = space("scoped-gate");
        let vectors = chunks
            .iter()
            .map(|chunk| {
                if chunk.document_id == "strong.md" {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                }
            })
            .collect::<Vec<_>>();
        retriever
            .vector_index
            .replace(space.clone(), chunks.clone(), vectors)
            .unwrap();
        let query = QueryEmbedding {
            space,
            vector: vec![1.0, 0.0],
        };
        assert!(retriever.evidence_gate(&query).unwrap().passed);
        let results = retriever
            .search_scoped(
                &documents,
                &chunks,
                "zzz",
                Some(&query),
                Some("target.md"),
                5,
            )
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn byte_identical_documents_do_not_use_extra_result_slots() {
        let source = InterimTextChunker::new(vec![
            document("archive/plan-copy.md", "deadline plan"),
            document("projects/plan.md", "deadline plan"),
            document("notes/tala.md", "deadline tala"),
        ]);
        let documents = source.documents();
        let chunks = source.all_chunks().unwrap();
        let mut retriever = HybridRetriever::default();
        let space = space("dedupe");
        let vectors = chunks
            .iter()
            .map(|chunk| {
                if chunk.document_id == "notes/tala.md" {
                    vec![0.8, 0.6]
                } else {
                    vec![1.0, 0.0]
                }
            })
            .collect::<Vec<_>>();
        retriever
            .vector_index
            .replace(space.clone(), chunks.clone(), vectors)
            .unwrap();
        let results = retriever
            .search(
                &documents,
                &chunks,
                "deadline",
                Some(&QueryEmbedding {
                    space,
                    vector: vec![1.0, 0.0],
                }),
                2,
            )
            .unwrap();
        let paths = results
            .iter()
            .map(|result| result.document.relative_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            ["archive/plan-copy.md", "projects/plan.md", "notes/tala.md"]
        );
    }

    #[test]
    fn keyword_overlap_cannot_outrank_a_clearly_stronger_semantic_match() {
        let source = InterimTextChunker::new(vec![
            document("distractor.md", "submission deadline project"),
            document("semantic.md", "huling araw ng pagpasa"),
        ]);
        let documents = source.documents();
        let chunks = source.all_chunks().unwrap();
        let mut retriever = HybridRetriever::default();
        let space = space("fusion");
        let vectors = chunks
            .iter()
            .map(|chunk| {
                if chunk.document_id == "semantic.md" {
                    vec![0.9, 0.435_889_9]
                } else {
                    vec![0.85, 0.526_782_7]
                }
            })
            .collect::<Vec<_>>();
        retriever
            .vector_index
            .replace(space.clone(), chunks.clone(), vectors)
            .unwrap();
        let results = retriever
            .search(
                &documents,
                &chunks,
                "submission deadline project",
                Some(&QueryEmbedding {
                    space,
                    vector: vec![1.0, 0.0],
                }),
                2,
            )
            .unwrap();
        assert_eq!(results[0].document.id, "semantic.md");
    }

    #[test]
    fn evidence_gate_uses_the_median_only_with_enough_chunks() {
        let flat = EvidenceGate::from_cosines(vec![0.80, 0.79, 0.79, 0.78, 0.78, 0.77]);
        assert!((flat.margin - 0.015).abs() < 1e-6);
        assert_eq!(flat.chunk_count, 6);
        let small = EvidenceGate::from_cosines(vec![0.9]);
        assert_eq!(small.margin, 0.0);
        assert_eq!(small.passed, small.top_cosine >= GATE_MIN_TOP_COSINE);
        let empty = EvidenceGate::from_cosines(Vec::new());
        assert!(!empty.passed);
    }

    #[test]
    fn admits_applies_the_gate_to_semantic_evidence_but_not_to_strong_keywords() {
        let open = EvidenceGate::from_cosines(vec![0.95, 0.80, 0.80, 0.80, 0.80, 0.80]);
        let closed = EvidenceGate::from_cosines(vec![0.80, 0.80, 0.80, 0.80, 0.80, 0.80]);
        assert!(open.passed && !closed.passed);
        assert!(admits(&open, MIN_SEMANTIC_SCORE, 0.0));
        assert!(!admits(&open, MIN_SEMANTIC_SCORE - 0.01, 0.0));
        assert!(!admits(&closed, 0.99, 0.0), "a closed gate blocks cosine");
        assert!(admits(&closed, 0.0, MIN_KEYWORD_SCORE));
        assert!(fused(0.8, 1.0) - 0.8 <= KEYWORD_TIEBREAK_WEIGHT + f32::EPSILON);
    }

    #[test]
    fn group_passages_keeps_pages_ranks_documents_and_lists_duplicates_beside_the_original() {
        let record = |id: &str, hash: &str| DocumentRecord {
            id: id.into(),
            workspace_id: "test-workspace".into(),
            relative_path: id.into(),
            name: id.into(),
            title: id.into(),
            language: Language::Mixed,
            media_type: "text/markdown".into(),
            size_bytes: 1,
            modified_at_ms: None,
            content: None,
            content_hash: Some(hash.into()),
        };
        let passage = |id: &str, hash: &str, start: usize, page: Option<u32>| SourcePassage {
            document_id: id.into(),
            document_content_hash: hash.into(),
            offset_unit: crate::contracts::OffsetUnit::Utf8Byte,
            start,
            end: start + 4,
            text: "text".into(),
            page,
        };
        let documents = [
            record("a.md", "sha256:a"),
            record("a-copy.md", "sha256:a"),
            record("b.md", "sha256:b"),
            record("c.md", "sha256:c"),
        ];
        let by_id = documents
            .iter()
            .map(|document| (document.id.as_str(), document))
            .collect();
        let results = group_passages(
            by_id,
            vec![
                (passage("a.md", "sha256:a", 0, Some(2)), 0.9),
                (passage("a-copy.md", "sha256:a", 0, None), 0.9),
                (passage("b.md", "sha256:b", 0, None), 0.8),
                (passage("c.md", "sha256:c", 0, None), 0.7),
                (passage("gone.md", "sha256:x", 0, None), 1.0),
            ],
            SearchMethod::Hybrid,
            Some("space".into()),
            3,
            2,
        );
        let ids = results
            .iter()
            .map(|result| result.document.id.as_str())
            .collect::<Vec<_>>();
        // The copy rides along with its original; two distinct contents fit.
        assert_eq!(ids, ["a-copy.md", "a.md", "b.md"]);
        let original = results.iter().find(|r| r.document.id == "a.md").unwrap();
        assert_eq!(original.passages[0].page, Some(2));
        assert!(results
            .iter()
            .all(|r| r.space_fingerprint.as_deref() == Some("space")));
    }

    #[test]
    fn changing_revision_refuses_cross_space_search() {
        let source = InterimTextChunker::new(vec![document("notes.md", "deadline")]);
        let chunks = source.all_chunks().unwrap();
        let mut index = VectorIndex::default();
        index
            .replace(space("a"), chunks, vec![vec![1.0, 0.0]])
            .unwrap();
        let error = index
            .search(
                &QueryEmbedding {
                    space: space("b"),
                    vector: vec![1.0, 0.0],
                },
                5,
            )
            .unwrap_err();
        assert!(error.to_string().contains("not indexed"));
    }

    #[test]
    fn provider_query_space_cannot_use_a_different_index_space() {
        let source = InterimTextChunker::new(vec![document("notes.md", "deadline")]);
        let chunks = source.all_chunks().unwrap();
        let mut index = VectorIndex::default();
        index
            .replace(space("provider-a"), chunks, vec![vec![1.0, 0.0]])
            .unwrap();
        let provider_b_query = QueryEmbedding {
            space: space("provider-b"),
            vector: vec![1.0, 0.0],
        };
        let error = index.search(&provider_b_query, 5).unwrap_err();
        assert!(error.to_string().contains("not indexed"));
    }

    #[test]
    fn scoped_search_applies_document_filter_before_limit() {
        let source = InterimTextChunker::new(vec![
            document("other.md", "other evidence"),
            document("target.md", "target evidence"),
        ]);
        let documents = source.documents();
        let chunks = source.all_chunks().unwrap();
        let mut retriever = HybridRetriever::default();
        let space = space("scoped");
        let vectors = chunks
            .iter()
            .map(|chunk| {
                if chunk.document_id == "other.md" {
                    vec![1.0, 0.0]
                } else {
                    // Above the calibrated top-cosine floor, so the scoped gate opens.
                    vec![0.9, 0.435_889_9]
                }
            })
            .collect::<Vec<_>>();
        retriever
            .vector_index
            .replace(space.clone(), chunks.clone(), vectors)
            .unwrap();
        let results = retriever
            .search_scoped(
                &documents,
                &chunks,
                "unrelated query",
                Some(&QueryEmbedding {
                    space,
                    vector: vec![1.0, 0.0],
                }),
                Some("target.md"),
                1,
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document.id, "target.md");
    }

    #[test]
    fn replacing_selected_space_drops_old_vectors() {
        let source = InterimTextChunker::new(vec![document("notes.md", "deadline")]);
        let chunks = source.all_chunks().unwrap();
        let mut index = VectorIndex::default();
        index
            .replace(space("a"), chunks.clone(), vec![vec![1.0, 0.0]])
            .unwrap();
        index
            .replace(space("b"), chunks, vec![vec![0.0, 1.0]])
            .unwrap();
        index.clear_except(&space("b"));
        assert_eq!(index.len_for(&space("a")), 0);
        assert_eq!(index.len_for(&space("b")), 1);
    }

    #[test]
    fn index_key_includes_preprocessing_fingerprint() {
        assert_ne!(
            embedding_space_id(&space("a")),
            embedding_space_id(&EmbeddingSpace {
                preprocessing_fingerprint: "chunk-v2".into(),
                ..space("a")
            })
        );
        assert!(content_hash("deadline").starts_with("sha256:"));
    }

    #[test]
    fn space_fingerprint_matches_the_frozen_identity_shape() {
        assert_eq!(
            space_fingerprint(&EmbeddingSpace {
                model_id: "a%2Fb".into(),
                revision: "r1".into(),
                quantization: "q8".into(),
                dimensions: 384,
                preprocessing_fingerprint: "raw-text".into(),
            }),
            "folio-space-v1/a%252Fb/r1/q8/384/raw-text"
        );
    }

    #[test]
    fn space_fingerprint_matches_contract_fixtures() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/contracts/contract-cases.json");
        let cases: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("contract fixture exists"))
                .expect("contract fixture is valid JSON");
        for entry in cases["identity"]["embeddingSpaceFingerprint"]
            .as_array()
            .unwrap()
        {
            let space = &entry["space"];
            assert_eq!(
                space_fingerprint(&EmbeddingSpace {
                    model_id: space["modelId"].as_str().unwrap().into(),
                    revision: space["revision"].as_str().unwrap().into(),
                    quantization: space["quantization"].as_str().unwrap().into(),
                    dimensions: space["dimensions"].as_u64().unwrap() as usize,
                    preprocessing_fingerprint: space["preprocessingFingerprint"]
                        .as_str()
                        .unwrap()
                        .into(),
                }),
                entry["expected"].as_str().unwrap()
            );
        }
    }
}
