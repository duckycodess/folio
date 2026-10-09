use crate::chunking::Chunk;
use crate::contracts::ProviderErrorCode;
use crate::contracts::{DocumentRecord, EmbeddingSpace, SearchMethod, SearchResult, SourcePassage};
use crate::embeddings::QueryEmbedding;
use crate::error::{CoreError, CoreResult, NativeProviderErrorError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

const RRF_K: f32 = 60.0;
const DEFAULT_PASSAGES_PER_DOCUMENT: usize = 3;

/// Provisional cosine gate calibrated only against the development corpus.
/// Revisit it when #3 supplies persisted retrieval evaluation data.
pub const MIN_SEMANTIC_SCORE: f32 = 0.35;
/// Keyword evidence may supplement semantic hits only when at least half of
/// the query terms occur in a chunk. This remains an interim fallback floor.
pub const MIN_KEYWORD_SCORE: f32 = 0.5;

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

impl HybridRetriever {
    pub fn keyword(
        &self,
        documents: &[DocumentRecord],
        chunks: &[Chunk],
        query: &str,
        limit: usize,
    ) -> Vec<SearchResult> {
        let terms = terms(query);
        if terms.is_empty() {
            return Vec::new();
        }
        let by_id = documents
            .iter()
            .map(|document| (document.id.as_str(), document))
            .collect::<HashMap<_, _>>();
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
            scored.push((chunk, score));
        }
        self.to_results(
            by_id,
            scored
                .into_iter()
                .map(|(chunk, score)| (chunk, score, score))
                .collect(),
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
            return Ok(self
                .keyword(documents, chunks, query, chunks.len())
                .into_iter()
                .filter(|result| document_id.is_none_or(|id| result.document.id == id))
                .take(limit)
                .collect());
        };
        let keyword = self
            .keyword(documents, chunks, query, chunks.len())
            .into_iter()
            .filter(|result| result.score >= MIN_KEYWORD_SCORE)
            .filter(|result| document_id.is_none_or(|id| result.document.id == id))
            .collect::<Vec<_>>();
        let semantic =
            self.vector_index
                .search_scoped(query_embedding, document_id, chunks.len())?;
        let semantic = semantic
            .into_iter()
            .filter(|(_, score)| *score >= MIN_SEMANTIC_SCORE)
            .collect::<Vec<_>>();
        let keyword_ranks = rank_by_chunk(&keyword, chunks);
        let mut combined = Vec::new();
        let mut seen = HashSet::new();
        for (rank, (chunk, score)) in semantic.iter().enumerate() {
            let key = (chunk.document_id.clone(), chunk.ordinal);
            seen.insert(key.clone());
            let keyword_score = keyword_ranks
                .get(&key)
                .map_or(0.0, |(keyword_rank, score)| {
                    let _ = keyword_rank;
                    *score
                });
            let keyword_rank_score = keyword_ranks.get(&key).map_or(0.0, |(keyword_rank, _)| {
                1.0 / (RRF_K + *keyword_rank as f32)
            });
            let semantic_rank_score = 1.0 / (RRF_K + (rank + 1) as f32);
            combined.push((
                chunk,
                *score,
                semantic_rank_score + keyword_rank_score + keyword_score * 0.001,
            ));
        }
        for (chunk, score) in keyword.iter().flat_map(|result| {
            result.passages.iter().filter_map(|passage| {
                chunks
                    .iter()
                    .find(|chunk| {
                        chunk.document_id == passage.document_id
                            && chunk.start == passage.start
                            && chunk.end == passage.end
                    })
                    .map(|chunk| (chunk, result.score))
            })
        }) {
            let key = (chunk.document_id.clone(), chunk.ordinal);
            if seen.insert(key) {
                combined.push((chunk, score, 1.0 / (RRF_K + 1.0) + score * 0.001));
            }
        }
        combined.sort_by(|a, b| b.2.total_cmp(&a.2));
        let by_id = documents
            .iter()
            .map(|document| (document.id.as_str(), document))
            .collect::<HashMap<_, _>>();
        Ok(self.to_results(
            by_id,
            combined,
            SearchMethod::Hybrid,
            Some(embedding_space_id(&query_embedding.space)),
            limit,
        ))
    }

    fn to_results<'a>(
        &self,
        by_id: HashMap<&'a str, &'a DocumentRecord>,
        scored: Vec<(&'a Chunk, f32, f32)>,
        method: SearchMethod,
        space_id: Option<String>,
        limit: usize,
    ) -> Vec<SearchResult> {
        let mut grouped: HashMap<String, (f32, Vec<SourcePassage>)> = HashMap::new();
        for (chunk, raw_score, result_score) in scored {
            let entry = grouped
                .entry(chunk.document_id.clone())
                .or_insert_with(|| (result_score, Vec::new()));
            entry.0 = entry.0.max(result_score);
            if entry.1.len() < self.max_passages {
                entry.1.push(SourcePassage {
                    document_id: chunk.document_id.clone(),
                    start: chunk.start,
                    end: chunk.end,
                    text: chunk.text.clone(),
                    page: None,
                });
            }
            let _ = raw_score;
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
                        embedding_space_id: space_id.clone(),
                    })
            })
            .collect::<Vec<_>>();
        results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.document.relative_path.cmp(&b.document.relative_path))
        });
        results.truncate(limit);
        results
    }
}

fn rank_by_chunk(
    results: &[SearchResult],
    chunks: &[Chunk],
) -> HashMap<(String, usize), (usize, f32)> {
    let mut rank = HashMap::new();
    for (result_rank, result) in results.iter().enumerate() {
        for passage in &result.passages {
            if let Some(chunk) = chunks.iter().find(|chunk| {
                chunk.document_id == passage.document_id
                    && chunk.start == passage.start
                    && chunk.end == passage.end
            }) {
                rank.insert(
                    (chunk.document_id.clone(), chunk.ordinal),
                    (result_rank + 1, result.score),
                );
            }
        }
    }
    rank
}

fn terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect::<HashSet<_>>()
        .into_iter()
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

pub fn embedding_space_id(space: &EmbeddingSpace) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunking::{sha256, ChunkSource, InterimTextChunker, TextDocument};
    use crate::contracts::{DocumentRecord, Language};

    fn document(id: &str, content: &str) -> TextDocument {
        TextDocument::new(
            DocumentRecord {
                id: id.into(),
                relative_path: id.into(),
                name: id.rsplit('/').next().unwrap_or(id).into(),
                title: id.into(),
                language: Language::Mixed,
                size_bytes: content.len() as u64,
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
                    vec![0.8, 0.6]
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
    fn space_id_includes_preprocessing_fingerprint() {
        assert_ne!(
            embedding_space_id(&space("a")),
            embedding_space_id(&EmbeddingSpace {
                preprocessing_fingerprint: "chunk-v2".into(),
                ..space("a")
            })
        );
        assert_eq!(sha256("deadline").len(), 64);
    }
}
