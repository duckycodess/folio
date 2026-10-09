//! Bounded, deterministic discovery over vectors already stored for one space.
//!
//! This module deliberately does not own embedding production or persistence.
//! The native index supplies fully embedded documents from one persistent space;
//! #27 can call that seam after its producer has populated the store.

use crate::contracts::{OffsetUnit, SourcePassage};
use crate::error::{CoreError, CoreResult};
use std::sync::atomic::{AtomicBool, Ordering};

/// Development-only gates. These are not calibrated product quality claims.
pub const MIN_SIMILARITY_COSINE: f32 = 0.80;
pub const MIN_SHARED_FACT_COSINE: f32 = 0.88;
pub const MAX_EVIDENCE_PER_SIDE: usize = 3;
/// Displayed (and Ripple-visible) AI edges per document and kind: an edge is
/// shown when it ranks within this many at either endpoint.
pub const MAX_AI_EDGES_PER_DOC: usize = 8;
/// Stored candidates per document and kind. Beyond this the weakest at that
/// endpoint is evicted and both of its endpoints are flagged as truncated.
pub const MAX_STORED_CANDIDATES_PER_ENDPOINT: usize = 32;
/// Keeps the vector payload bounded before pair discovery starts.
pub const MAX_RELATIONSHIP_CHUNKS: usize = 20_000;
/// Keeps worst-case CPU bounded; a refresh fails rather than silently dropping
/// comparisons when the corpus is too large for this draft implementation.
pub const MAX_RELATIONSHIP_PAIR_COMPARISONS: usize = 250_000;
/// Keeps the transient discovered-edge list bounded before persistence.
pub const MAX_DISCOVERED_RELATIONSHIPS: usize = 20_000;

#[derive(Clone, Debug)]
pub struct RelationshipChunk {
    pub document_id: String,
    pub document_content_hash: String,
    pub start: usize,
    pub end: usize,
    pub page: Option<u32>,
    pub text: String,
    pub vector: Vec<f32>,
}

impl RelationshipChunk {
    fn passage(&self, start: usize, end: usize, text: &str) -> SourcePassage {
        SourcePassage {
            document_id: self.document_id.clone(),
            document_content_hash: self.document_content_hash.clone(),
            offset_unit: OffsetUnit::Utf8Byte,
            start,
            end,
            text: text.to_owned(),
            page: self.page,
        }
    }

    fn full_passage(&self) -> SourcePassage {
        self.passage(self.start, self.end, &self.text)
    }
}

#[derive(Clone, Debug)]
pub struct RelationshipDocument {
    pub id: String,
    pub content_hash: String,
    pub chunks: Vec<RelationshipChunk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AiRelationshipKind {
    Similarity,
    SharedFactCandidate,
}

#[derive(Clone, Debug)]
pub struct DiscoveredRelationship {
    pub kind: AiRelationshipKind,
    pub source_id: String,
    pub target_id: String,
    pub source_content_hash: String,
    pub target_content_hash: String,
    pub space_fingerprint: String,
    pub score: Option<f32>,
    pub confidence: Option<f32>,
    /// Internal ranking cosine for retained candidates; never on the wire.
    pub discovery_cosine: f32,
    pub source_evidence: Vec<SourcePassage>,
    pub target_evidence: Vec<SourcePassage>,
}

#[derive(Clone, Debug)]
struct PairMatch {
    score: f32,
    left_chunk: usize,
    right_chunk: usize,
}

#[derive(Clone, Debug)]
struct Anchor {
    key: String,
    start: usize,
    end: usize,
}

/// Discover relationships from persistent vectors belonging to one space.
///
/// The caller only supplies documents that are complete in that space. Every
/// document pair is compared once, with explicit limits on total chunks and
/// vector-pair CPU work. Byte-identical documents are left to duplicate
/// detection and therefore never become AI relationships.
pub fn discover(
    documents: &[RelationshipDocument],
    space_fingerprint: &str,
    cancel: Option<&AtomicBool>,
) -> CoreResult<Vec<DiscoveredRelationship>> {
    if space_fingerprint.trim().is_empty() {
        return Err(CoreError::Message(
            "Relationship discovery needs an embedding-space fingerprint.".into(),
        ));
    }
    let total_chunks: usize = documents.iter().map(|document| document.chunks.len()).sum();
    if total_chunks > MAX_RELATIONSHIP_CHUNKS {
        return Err(CoreError::Message(format!(
            "Relationship discovery is limited to {MAX_RELATIONSHIP_CHUNKS} chunks in this version."
        )));
    }

    let mut comparisons = 0_usize;
    let mut result = Vec::new();
    for left_index in 0..documents.len() {
        check_cancel(cancel)?;
        let left = &documents[left_index];
        for right in documents.iter().skip(left_index + 1) {
            check_cancel(cancel)?;
            if left.content_hash == right.content_hash
                || left.chunks.is_empty()
                || right.chunks.is_empty()
            {
                continue;
            }

            let mut best: Option<PairMatch> = None;
            let mut top_matches = Vec::new();
            let mut shared: Option<(PairMatch, Anchor, Anchor)> = None;
            for (left_chunk_index, left_chunk) in left.chunks.iter().enumerate() {
                for (right_chunk_index, right_chunk) in right.chunks.iter().enumerate() {
                    comparisons += 1;
                    if comparisons > MAX_RELATIONSHIP_PAIR_COMPARISONS {
                        return Err(CoreError::Message(format!(
                            "Relationship discovery is limited to {MAX_RELATIONSHIP_PAIR_COMPARISONS} vector comparisons in this version."
                        )));
                    }
                    let score = cosine(&left_chunk.vector, &right_chunk.vector)?;
                    let pair = PairMatch {
                        score,
                        left_chunk: left_chunk_index,
                        right_chunk: right_chunk_index,
                    };
                    if best.as_ref().is_none_or(|current| score > current.score) {
                        best = Some(pair.clone());
                    }
                    if score >= MIN_SIMILARITY_COSINE {
                        push_top(&mut top_matches, pair.clone());
                    }
                    if score >= MIN_SHARED_FACT_COSINE {
                        if let Some((left_anchor, right_anchor)) =
                            shared_anchor(&left_chunk.text, &right_chunk.text)
                        {
                            if shared
                                .as_ref()
                                .is_none_or(|(current, _, _)| score > current.score)
                            {
                                shared = Some((pair, left_anchor, right_anchor));
                            }
                        }
                    }
                }
            }

            let Some(best) = best else { continue };
            if best.score >= MIN_SIMILARITY_COSINE {
                let mut source_evidence = Vec::new();
                let mut target_evidence = Vec::new();
                for pair in &top_matches {
                    let left_passage = left.chunks[pair.left_chunk].full_passage();
                    let right_passage = right.chunks[pair.right_chunk].full_passage();
                    push_unique(&mut source_evidence, left_passage);
                    push_unique(&mut target_evidence, right_passage);
                }
                if !source_evidence.is_empty() && !target_evidence.is_empty() {
                    let (source_id, target_id, source_hash, target_hash, source_evidence, target_evidence) =
                        canonical_pair(left, right, source_evidence, target_evidence);
                    result.push(DiscoveredRelationship {
                        kind: AiRelationshipKind::Similarity,
                        source_id,
                        target_id,
                        source_content_hash: source_hash,
                        target_content_hash: target_hash,
                        space_fingerprint: space_fingerprint.to_owned(),
                        score: Some(best.score.clamp(0.0, 1.0)),
                        confidence: None,
                        discovery_cosine: best.score.clamp(0.0, 1.0),
                        source_evidence,
                        target_evidence,
                    });
                    if result.len() > MAX_DISCOVERED_RELATIONSHIPS {
                        return Err(CoreError::Message(format!(
                            "Relationship discovery is limited to {MAX_DISCOVERED_RELATIONSHIPS} candidate edges in this version."
                        )));
                    }
                }
            }

            if let Some((pair, left_anchor, right_anchor)) = shared {
                let left_passage = narrowed_passage(&left.chunks[pair.left_chunk], &left_anchor);
                let right_passage = narrowed_passage(&right.chunks[pair.right_chunk], &right_anchor);
                let (source_id, target_id, source_hash, target_hash, source_evidence, target_evidence) =
                    canonical_pair(left, right, vec![left_passage], vec![right_passage]);
                result.push(DiscoveredRelationship {
                    kind: AiRelationshipKind::SharedFactCandidate,
                    source_id,
                    target_id,
                    source_content_hash: source_hash,
                    target_content_hash: target_hash,
                    space_fingerprint: space_fingerprint.to_owned(),
                    score: None,
                    confidence: None,
                    discovery_cosine: pair.score.clamp(0.0, 1.0),
                    source_evidence,
                    target_evidence,
                });
                if result.len() > MAX_DISCOVERED_RELATIONSHIPS {
                    return Err(CoreError::Message(format!(
                        "Relationship discovery is limited to {MAX_DISCOVERED_RELATIONSHIPS} candidate edges in this version."
                    )));
                }
            }
        }
    }

    cap_edges(&mut result);
    Ok(result)
}

fn check_cancel(cancel: Option<&AtomicBool>) -> CoreResult<()> {
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(CoreError::Message("relationship discovery cancelled".into()));
    }
    Ok(())
}

fn cosine(left: &[f32], right: &[f32]) -> CoreResult<f32> {
    if left.is_empty() || left.len() != right.len() {
        return Err(CoreError::Message(
            "A relationship vector dimension does not match its space.".into(),
        ));
    }
    if left.iter().chain(right).any(|value| !value.is_finite()) {
        return Err(CoreError::Message(
            "Relationship vectors must contain finite numbers.".into(),
        ));
    }
    let dot = left.iter().zip(right).map(|(a, b)| a * b).sum::<f32>();
    let left_norm = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    Ok(if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm * right_norm)
    })
}

fn push_top(matches: &mut Vec<PairMatch>, candidate: PairMatch) {
    matches.push(candidate);
    matches.sort_by(|left, right| right.score.total_cmp(&left.score));
    matches.truncate(MAX_EVIDENCE_PER_SIDE * 2);
}

fn push_unique(passages: &mut Vec<SourcePassage>, passage: SourcePassage) {
    if passages.iter().any(|existing| {
        existing.document_id == passage.document_id
            && existing.start == passage.start
            && existing.end == passage.end
    }) {
        return;
    }
    if passages.len() < MAX_EVIDENCE_PER_SIDE {
        passages.push(passage);
    }
}

fn canonical_pair(
    left: &RelationshipDocument,
    right: &RelationshipDocument,
    left_evidence: Vec<SourcePassage>,
    right_evidence: Vec<SourcePassage>,
) -> (String, String, String, String, Vec<SourcePassage>, Vec<SourcePassage>) {
    if left.id <= right.id {
        (
            left.id.clone(),
            right.id.clone(),
            left.content_hash.clone(),
            right.content_hash.clone(),
            left_evidence,
            right_evidence,
        )
    } else {
        (
            right.id.clone(),
            left.id.clone(),
            right.content_hash.clone(),
            left.content_hash.clone(),
            right_evidence,
            left_evidence,
        )
    }
}

fn cap_edges(edges: &mut Vec<DiscoveredRelationship>) {
    let mut counts: std::collections::BTreeMap<(AiRelationshipKind, String), usize> =
        std::collections::BTreeMap::new();
    edges.retain(|edge| {
        let key = (edge.kind, edge.source_id.clone());
        let count = counts.entry(key).or_default();
        if *count >= MAX_AI_EDGES_PER_DOC {
            return false;
        }
        *count += 1;
        true
    });
}

fn shared_anchor(left: &str, right: &str) -> Option<(Anchor, Anchor)> {
    let left_anchors = anchors(left);
    let right_anchors = anchors(right);
    left_anchors.iter().find_map(|left_anchor| {
        right_anchors
            .iter()
            .find(|right_anchor| right_anchor.key == left_anchor.key)
            .map(|right_anchor| (left_anchor.clone(), right_anchor.clone()))
    })
}

fn anchors(text: &str) -> Vec<Anchor> {
    let tokens = token_spans(text);
    let mut found = Vec::new();
    for (index, (start, end, token)) in tokens.iter().enumerate() {
        if let Some(month) = month_index(token) {
            if let Some((_, next_end, day)) = tokens.get(index + 1) {
                if is_day(day) {
                    found.push(Anchor {
                        key: format!("date:{month}:{day}"),
                        start: *start,
                        end: *next_end,
                    });
                }
            }
        }
        let digits = token.trim_matches(|character: char| !character.is_ascii_digit());
        if digits.len() >= 2 && digits.chars().all(|character| character.is_ascii_digit()) {
            found.push(Anchor {
                key: format!("number:{digits}"),
                start: *start,
                end: *end,
            });
        }
    }
    found
}

fn token_spans(text: &str) -> Vec<(usize, usize, String)> {
    text.split_whitespace()
        .scan(0_usize, |cursor, token| {
            let start = text[*cursor..].find(token)? + *cursor;
            *cursor = start + token.len();
            Some((start, start + token.len(), token.to_owned()))
        })
        .collect()
}

fn clean_token(token: &str) -> String {
    token
        .trim_matches(|character: char| !character.is_alphanumeric())
        .to_lowercase()
}

fn month_index(token: &str) -> Option<usize> {
    let token = clean_token(token);
    [
        ["january", "jan", "enero", ""],
        ["february", "feb", "pebrero", ""],
        ["march", "mar", "marso", ""],
        ["april", "apr", "abril", ""],
        ["may", "mayo", "", ""],
        ["june", "jun", "hunyo", ""],
        ["july", "jul", "hulyo", ""],
        ["august", "aug", "agosto", ""],
        ["september", "sep", "setyembre", ""],
        ["october", "oct", "oktubre", "octubre"],
        ["november", "nov", "nobyembre", ""],
        ["december", "dec", "disyembre", ""],
    ]
    .iter()
    .position(|names| names.iter().any(|name| *name == token))
}

fn is_day(token: &str) -> bool {
    let token = token.trim_matches(|character: char| !character.is_ascii_digit());
    (1..=2).contains(&token.len()) && token.chars().all(|character| character.is_ascii_digit())
}

fn narrowed_passage(chunk: &RelationshipChunk, anchor: &Anchor) -> SourcePassage {
    let (start, end) = sentence_bounds(&chunk.text, anchor.start, anchor.end);
    chunk.passage(chunk.start + start, chunk.start + end, &chunk.text[start..end])
}

fn sentence_bounds(text: &str, start: usize, end: usize) -> (usize, usize) {
    let before = text[..start]
        .char_indices()
        .rev()
        .find(|(_, character)| matches!(character, '.' | '!' | '?' | '\n'))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let after = text[end..]
        .char_indices()
        .find(|(_, character)| matches!(character, '.' | '!' | '?' | '\n'))
        .map_or(text.len(), |(index, character)| end + index + character.len_utf8());
    let leading = text[before..after].len() - text[before..after].trim_start().len();
    let trailing = text[before..after].len() - text[before..after].trim_end().len();
    (before + leading, after - trailing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(id: &str, hash: &str, text: &str, vector: Vec<f32>) -> RelationshipDocument {
        RelationshipDocument {
            id: id.into(),
            content_hash: hash.into(),
            chunks: vec![RelationshipChunk {
                document_id: id.into(),
                document_content_hash: hash.into(),
                start: 0,
                end: text.len(),
                page: None,
                text: text.into(),
                vector,
            }],
        }
    }

    #[test]
    fn similarity_is_evidenced_and_space_bound() {
        let edges = discover(
            &[
                document("a", "sha256:a", "Project deadline October 20.", vec![1.0, 0.0]),
                document("b", "sha256:b", "Huling araw October 20.", vec![0.99, 0.01]),
            ],
            "folio-space-v1/test",
            None,
        )
        .unwrap();
        let similarity = edges
            .iter()
            .find(|edge| edge.kind == AiRelationshipKind::Similarity)
            .unwrap();
        assert_eq!(similarity.space_fingerprint, "folio-space-v1/test");
        assert!(similarity.score.unwrap() <= 1.0);
        assert!(!similarity.source_evidence.is_empty());
        assert!(!similarity.target_evidence.is_empty());
    }

    #[test]
    fn shared_fact_requires_a_shared_date_and_narrows_to_utf8_sentence() {
        let edges = discover(
            &[
                document("a", "sha256:a", "Paalala: deadline Oktubre 20. Iba pa.", vec![1.0, 0.0]),
                document("b", "sha256:b", "The deadline is October 20. Next.", vec![1.0, 0.0]),
            ],
            "space",
            None,
        )
        .unwrap();
        let shared = edges
            .iter()
            .find(|edge| edge.kind == AiRelationshipKind::SharedFactCandidate)
            .unwrap();
        assert_eq!(shared.confidence, None);
        assert!(shared.source_evidence[0].text.contains("Oktubre 20"));
        assert!(shared.target_evidence[0].text.contains("October 20"));
        assert!(shared.source_evidence[0].text.is_char_boundary(shared.source_evidence[0].text.len()));
    }

    #[test]
    fn high_cosine_without_an_anchor_is_not_a_shared_fact() {
        let edges = discover(
            &[
                document("a", "sha256:a", "The project is ready.", vec![1.0, 0.0]),
                document("b", "sha256:b", "The research is ready.", vec![1.0, 0.0]),
            ],
            "space",
            None,
        )
        .unwrap();
        assert!(edges
            .iter()
            .all(|edge| edge.kind != AiRelationshipKind::SharedFactCandidate));
    }

    #[test]
    fn byte_identical_documents_are_not_ai_edges() {
        let edges = discover(
            &[
                document("a", "sha256:same", "same", vec![1.0, 0.0]),
                document("b", "sha256:same", "same", vec![1.0, 0.0]),
            ],
            "space",
            None,
        )
        .unwrap();
        assert!(edges.is_empty());
    }

    #[test]
    fn pair_comparisons_are_bounded_before_all_pairs_complete() {
        let count = 708;
        let documents = (0..count)
            .map(|index| {
                let mut vector = vec![0.0; count];
                vector[index] = 1.0;
                document(
                    &format!("doc-{index}"),
                    &format!("sha256:{index:064x}"),
                    &format!("Unrelated note {index}."),
                    vector,
                )
            })
            .collect::<Vec<_>>();

        let error = discover(&documents, "space", None).unwrap_err();

        assert!(error
            .to_string()
            .contains("250000 vector comparisons"));
    }

    #[test]
    fn discovered_edges_have_a_transient_memory_bound() {
        let documents = (0..202)
            .map(|index| {
                document(
                    &format!("doc-{index}"),
                    &format!("sha256:{index:064x}"),
                    "The deadline is October 20.",
                    vec![1.0, 0.0],
                )
            })
            .collect::<Vec<_>>();

        let error = discover(&documents, "space", None).unwrap_err();

        assert!(error.to_string().contains("candidate edges"));
    }
}
