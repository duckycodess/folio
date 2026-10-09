//! Bounded, deterministic relationship discovery over vectors already stored
//! for one space.
//!
//! This module owns no persistence and no scheduling. The native index feeds it
//! one bounded *tile* of a document pair at a time (rows are one document's
//! chunks, columns the other's), keeps the small `PairAccumulator` between
//! tiles, and asks `finish_pair` for the candidate edges once the last tile of
//! a pair is done. Nothing here depends on how tiles are scheduled, so an
//! interrupted and resumed pair equals an uninterrupted one.

use crate::contracts::{OffsetUnit, SourcePassage};
use crate::error::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
/// One tile is at most this many chunks of the job's document by this many of
/// its partner's, so a single huge pair still advances in bounded steps.
pub const TILE_ROWS: usize = 32;
pub const TILE_COLS: usize = 64;
pub const MAX_TILE_COMPARISONS: usize = TILE_ROWS * TILE_COLS;
/// Vector comparisons in one discovery run. Never below one tile, so every
/// run makes progress.
pub const MAX_RUN_COMPARISONS: usize = 250_000;
/// Tiles one job may take before the scheduler moves to the next job.
pub const TILES_PER_JOB_PER_TURN: usize = 2;

const _: () = assert!(MAX_RUN_COMPARISONS >= MAX_TILE_COMPARISONS);

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

/// A chunk pair by position within each document (row-major tile order is
/// irrelevant: ties are broken by position, never by arrival order). The
/// cosine is kept as its bit pattern so a persisted accumulator round-trips
/// exactly.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChunkPairRef {
    pub cosine_bits: u32,
    pub left: u32,
    pub right: u32,
}

impl ChunkPairRef {
    fn cosine(&self) -> f32 {
        f32::from_bits(self.cosine_bits)
    }

    fn outranks(&self, other: &Self) -> bool {
        self.cosine()
            .total_cmp(&other.cosine())
            .then(other.left.cmp(&self.left))
            .then(other.right.cmp(&self.right))
            .is_gt()
    }
}

/// The best shared-fact reference of a pair: the chunk pair and the narrowed
/// byte spans (within each chunk's text) that carry the shared anchor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SharedFactRef {
    pub pair: ChunkPairRef,
    pub left_start: u32,
    pub left_end: u32,
    pub right_start: u32,
    pub right_end: u32,
}

/// Everything a half-finished pair has learned, bounded in size: the best
/// cosine, a few chunk-pair references and one shared-fact reference.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PairAccumulator {
    pub best: Option<ChunkPairRef>,
    pub top_pairs: Vec<ChunkPairRef>,
    pub shared: Option<SharedFactRef>,
}

impl PairAccumulator {
    fn observe(&mut self, candidate: ChunkPairRef) {
        if self
            .best
            .as_ref()
            .is_none_or(|best| candidate.outranks(best))
        {
            self.best = Some(candidate);
        }
        if candidate.cosine() >= MIN_SIMILARITY_COSINE {
            let position = self
                .top_pairs
                .iter()
                .position(|existing| candidate.outranks(existing))
                .unwrap_or(self.top_pairs.len());
            if position < MAX_EVIDENCE_PER_SIDE * 2 {
                self.top_pairs.insert(position, candidate);
                self.top_pairs.truncate(MAX_EVIDENCE_PER_SIDE * 2);
            }
        }
    }

    fn observe_shared(&mut self, candidate: SharedFactRef) {
        if self
            .shared
            .as_ref()
            .is_none_or(|current| candidate.pair.outranks(&current.pair))
        {
            self.shared = Some(candidate);
        }
    }
}

/// Compares one tile: `left` and `right` are consecutive chunks of the two
/// documents starting at positions `left_start` and `right_start`. Returns the
/// number of vector comparisons made. Cancellation is checked per row; the
/// caller discards a half-processed accumulator by never persisting it.
pub fn process_tile(
    accumulator: &mut PairAccumulator,
    left: &[RelationshipChunk],
    left_start: usize,
    right: &[RelationshipChunk],
    right_start: usize,
    cancel: Option<&AtomicBool>,
) -> CoreResult<usize> {
    if left.len() * right.len() > MAX_TILE_COMPARISONS {
        return Err(CoreError::Message(format!(
            "A relationship tile is limited to {MAX_TILE_COMPARISONS} comparisons."
        )));
    }
    let left_norms = norms(left)?;
    let right_norms = norms(right)?;
    for (row, left_chunk) in left.iter().enumerate() {
        check_cancel(cancel)?;
        for (column, right_chunk) in right.iter().enumerate() {
            if left_chunk.vector.len() != right_chunk.vector.len() {
                return Err(CoreError::Message(
                    "A relationship vector dimension does not match its space.".into(),
                ));
            }
            let dot = left_chunk
                .vector
                .iter()
                .zip(&right_chunk.vector)
                .map(|(a, b)| a * b)
                .sum::<f32>();
            let cosine = if left_norms[row] == 0.0 || right_norms[column] == 0.0 {
                0.0
            } else {
                (dot / (left_norms[row] * right_norms[column])).clamp(-1.0, 1.0)
            };
            let pair = ChunkPairRef {
                cosine_bits: cosine.to_bits(),
                left: (left_start + row) as u32,
                right: (right_start + column) as u32,
            };
            accumulator.observe(pair);
            if cosine >= MIN_SHARED_FACT_COSINE {
                if let Some((left_anchor, right_anchor)) =
                    shared_anchor(&left_chunk.text, &right_chunk.text)
                {
                    let (left_from, left_to) =
                        sentence_bounds(&left_chunk.text, left_anchor.start, left_anchor.end);
                    let (right_from, right_to) =
                        sentence_bounds(&right_chunk.text, right_anchor.start, right_anchor.end);
                    accumulator.observe_shared(SharedFactRef {
                        pair,
                        left_start: left_from as u32,
                        left_end: left_to as u32,
                        right_start: right_from as u32,
                        right_end: right_to as u32,
                    });
                }
            }
        }
    }
    Ok(left.len() * right.len())
}

/// One document's side of a finished pair: only the chunks the accumulator
/// references need to be loaded (vectors are not used here).
#[derive(Clone, Debug)]
pub struct PairSide {
    pub id: String,
    pub content_hash: String,
    pub chunks: BTreeMap<usize, RelationshipChunk>,
}

impl PairSide {
    fn chunk(&self, position: u32) -> CoreResult<&RelationshipChunk> {
        self.chunks.get(&(position as usize)).ok_or_else(|| {
            CoreError::Message("A relationship evidence chunk is no longer available.".into())
        })
    }
}

/// The positions the accumulator references in the left and right document.
pub fn referenced_positions(accumulator: &PairAccumulator) -> (Vec<u32>, Vec<u32>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for pair in accumulator
        .top_pairs
        .iter()
        .chain(accumulator.shared.iter().map(|shared| &shared.pair))
    {
        left.push(pair.left);
        right.push(pair.right);
    }
    left.sort_unstable();
    left.dedup();
    right.sort_unstable();
    right.dedup();
    (left, right)
}

/// Candidate edges of a finished pair. `left` is the job's document and
/// `right` its partner; the edge's source is the lexicographically smaller id.
pub fn finish_pair(
    left: &PairSide,
    right: &PairSide,
    accumulator: &PairAccumulator,
    space_fingerprint: &str,
) -> CoreResult<Vec<DiscoveredRelationship>> {
    if space_fingerprint.trim().is_empty() {
        return Err(CoreError::Message(
            "Relationship discovery needs an embedding-space fingerprint.".into(),
        ));
    }
    let mut edges = Vec::new();
    if let Some(best) = accumulator
        .best
        .filter(|best| best.cosine() >= MIN_SIMILARITY_COSINE)
    {
        let mut left_evidence = Vec::new();
        let mut right_evidence = Vec::new();
        for pair in &accumulator.top_pairs {
            push_unique(&mut left_evidence, left.chunk(pair.left)?.full_passage());
            push_unique(&mut right_evidence, right.chunk(pair.right)?.full_passage());
        }
        if !left_evidence.is_empty() && !right_evidence.is_empty() {
            let score = best.cosine().clamp(0.0, 1.0);
            let (source, target, source_evidence, target_evidence) =
                canonical_pair(left, right, left_evidence, right_evidence);
            edges.push(DiscoveredRelationship {
                kind: AiRelationshipKind::Similarity,
                source_id: source.id.clone(),
                target_id: target.id.clone(),
                source_content_hash: source.content_hash.clone(),
                target_content_hash: target.content_hash.clone(),
                space_fingerprint: space_fingerprint.to_owned(),
                score: Some(score),
                confidence: None,
                discovery_cosine: score,
                source_evidence,
                target_evidence,
            });
        }
    }
    if let Some(shared) = accumulator.shared {
        let left_chunk = left.chunk(shared.pair.left)?;
        let right_chunk = right.chunk(shared.pair.right)?;
        let left_passage = narrowed(left_chunk, shared.left_start, shared.left_end)?;
        let right_passage = narrowed(right_chunk, shared.right_start, shared.right_end)?;
        let (source, target, source_evidence, target_evidence) =
            canonical_pair(left, right, vec![left_passage], vec![right_passage]);
        edges.push(DiscoveredRelationship {
            kind: AiRelationshipKind::SharedFactCandidate,
            source_id: source.id.clone(),
            target_id: target.id.clone(),
            source_content_hash: source.content_hash.clone(),
            target_content_hash: target.content_hash.clone(),
            space_fingerprint: space_fingerprint.to_owned(),
            score: None,
            confidence: None,
            discovery_cosine: shared.pair.cosine().clamp(0.0, 1.0),
            source_evidence,
            target_evidence,
        });
    }
    Ok(edges)
}

fn narrowed(chunk: &RelationshipChunk, start: u32, end: u32) -> CoreResult<SourcePassage> {
    let (start, end) = (start as usize, end as usize);
    if start >= end
        || end > chunk.text.len()
        || !chunk.text.is_char_boundary(start)
        || !chunk.text.is_char_boundary(end)
    {
        return Err(CoreError::Message(
            "A shared-fact span no longer fits its chunk.".into(),
        ));
    }
    Ok(chunk.passage(
        chunk.start + start,
        chunk.start + end,
        &chunk.text[start..end],
    ))
}

fn check_cancel(cancel: Option<&AtomicBool>) -> CoreResult<()> {
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(CoreError::Message(
            "relationship discovery cancelled".into(),
        ));
    }
    Ok(())
}

fn norms(chunks: &[RelationshipChunk]) -> CoreResult<Vec<f32>> {
    chunks
        .iter()
        .map(|chunk| {
            if chunk.vector.is_empty() {
                return Err(CoreError::Message(
                    "A relationship vector dimension does not match its space.".into(),
                ));
            }
            if chunk.vector.iter().any(|value| !value.is_finite()) {
                return Err(CoreError::Message(
                    "Relationship vectors must contain finite numbers.".into(),
                ));
            }
            Ok(chunk
                .vector
                .iter()
                .map(|value| value * value)
                .sum::<f32>()
                .sqrt())
        })
        .collect()
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

fn canonical_pair<'a>(
    left: &'a PairSide,
    right: &'a PairSide,
    left_evidence: Vec<SourcePassage>,
    right_evidence: Vec<SourcePassage>,
) -> (
    &'a PairSide,
    &'a PairSide,
    Vec<SourcePassage>,
    Vec<SourcePassage>,
) {
    if left.id <= right.id {
        (left, right, left_evidence, right_evidence)
    } else {
        (right, left, right_evidence, left_evidence)
    }
}

#[derive(Clone, Debug)]
struct Anchor {
    key: String,
    start: usize,
    end: usize,
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

fn sentence_bounds(text: &str, start: usize, end: usize) -> (usize, usize) {
    let before = text[..start]
        .char_indices()
        .rev()
        .find(|(_, character)| matches!(character, '.' | '!' | '?' | '\n'))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let after = text[end..]
        .char_indices()
        .find(|(_, character)| matches!(character, '.' | '!' | '?' | '\n'))
        .map_or(text.len(), |(index, character)| {
            end + index + character.len_utf8()
        });
    let leading = text[before..after].len() - text[before..after].trim_start().len();
    let trailing = text[before..after].len() - text[before..after].trim_end().len();
    (before + leading, after - trailing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &str, hash: &str, text: &str, vector: Vec<f32>) -> RelationshipChunk {
        RelationshipChunk {
            document_id: id.into(),
            document_content_hash: hash.into(),
            start: 0,
            end: text.len(),
            page: None,
            text: text.into(),
            vector,
        }
    }

    fn side(id: &str, chunks: Vec<RelationshipChunk>) -> PairSide {
        PairSide {
            id: id.into(),
            content_hash: format!("sha256:{id}"),
            chunks: chunks.into_iter().enumerate().collect(),
        }
    }

    /// Runs a whole pair through tiles of the given shape, optionally
    /// round-tripping the accumulator through JSON between tiles.
    fn run_pair(
        left: &PairSide,
        right: &PairSide,
        tile_rows: usize,
        tile_cols: usize,
        persist_between_tiles: bool,
    ) -> (PairAccumulator, usize) {
        let left_chunks: Vec<_> = left.chunks.values().cloned().collect();
        let right_chunks: Vec<_> = right.chunks.values().cloned().collect();
        let mut accumulator = PairAccumulator::default();
        let mut comparisons = 0;
        let mut row = 0;
        while row < left_chunks.len() {
            let mut column = 0;
            while column < right_chunks.len() {
                let rows = &left_chunks[row..(row + tile_rows).min(left_chunks.len())];
                let columns = &right_chunks[column..(column + tile_cols).min(right_chunks.len())];
                if persist_between_tiles {
                    accumulator =
                        serde_json::from_str(&serde_json::to_string(&accumulator).unwrap())
                            .unwrap();
                }
                comparisons +=
                    process_tile(&mut accumulator, rows, row, columns, column, None).unwrap();
                column += tile_cols;
            }
            row += tile_rows;
        }
        (accumulator, comparisons)
    }

    fn finish(
        left: &PairSide,
        right: &PairSide,
        accumulator: &PairAccumulator,
    ) -> Vec<DiscoveredRelationship> {
        finish_pair(left, right, accumulator, "folio-space-v1/test").unwrap()
    }

    #[test]
    fn similarity_is_evidenced_and_space_bound() {
        let a = side(
            "a",
            vec![chunk(
                "a",
                "sha256:a",
                "Project deadline October 20.",
                vec![1.0, 0.0],
            )],
        );
        let b = side(
            "b",
            vec![chunk(
                "b",
                "sha256:b",
                "Huling araw October 20.",
                vec![0.99, 0.01],
            )],
        );
        let (accumulator, comparisons) = run_pair(&a, &b, 8, 8, false);
        assert_eq!(comparisons, 1);
        let edges = finish(&a, &b, &accumulator);
        let similarity = edges
            .iter()
            .find(|edge| edge.kind == AiRelationshipKind::Similarity)
            .unwrap();
        assert_eq!(similarity.space_fingerprint, "folio-space-v1/test");
        assert!(similarity.score.unwrap() <= 1.0);
        assert_eq!(similarity.discovery_cosine, similarity.score.unwrap());
        assert!(!similarity.source_evidence.is_empty());
        assert!(!similarity.target_evidence.is_empty());
    }

    #[test]
    fn shared_fact_requires_a_shared_date_and_narrows_to_utf8_sentence() {
        let a = side(
            "a",
            vec![chunk(
                "a",
                "sha256:a",
                "Paalala: deadline Oktubre 20. Iba pa.",
                vec![1.0, 0.0],
            )],
        );
        let b = side(
            "b",
            vec![chunk(
                "b",
                "sha256:b",
                "The deadline is October 20. Next.",
                vec![1.0, 0.0],
            )],
        );
        let (accumulator, _) = run_pair(&a, &b, 8, 8, false);
        let edges = finish(&a, &b, &accumulator);
        let shared = edges
            .iter()
            .find(|edge| edge.kind == AiRelationshipKind::SharedFactCandidate)
            .unwrap();
        assert_eq!(shared.confidence, None);
        assert!(shared.source_evidence[0].text.contains("Oktubre 20"));
        assert!(shared.target_evidence[0].text.contains("October 20"));
    }

    #[test]
    fn high_cosine_without_an_anchor_is_not_a_shared_fact() {
        let a = side(
            "a",
            vec![chunk(
                "a",
                "sha256:a",
                "The project is ready.",
                vec![1.0, 0.0],
            )],
        );
        let b = side(
            "b",
            vec![chunk(
                "b",
                "sha256:b",
                "The research is ready.",
                vec![1.0, 0.0],
            )],
        );
        let (accumulator, _) = run_pair(&a, &b, 8, 8, false);
        assert!(finish(&a, &b, &accumulator)
            .iter()
            .all(|edge| edge.kind != AiRelationshipKind::SharedFactCandidate));
    }

    fn big_side(id: &str, chunks: usize, seed: usize) -> PairSide {
        side(
            id,
            (0..chunks)
                .map(|index| {
                    // Many near-equal cosines so ties and eviction order matter.
                    let angle = ((index * 7 + seed * 3) % 11) as f32 / 40.0;
                    chunk(
                        id,
                        &format!("sha256:{id}"),
                        &format!("Chunk {index} of {id} mentions October {}.", 10 + index % 3),
                        vec![angle.cos(), angle.sin()],
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn tiling_and_persisting_the_accumulator_never_changes_the_result() {
        let left = big_side("left", 70, 1);
        let right = big_side("right", 130, 2);
        let (whole, whole_comparisons) = run_pair(&left, &right, 70, 1, false);
        assert_eq!(whole_comparisons, 70 * 130);
        assert!(
            whole.top_pairs.len() <= MAX_EVIDENCE_PER_SIDE * 2,
            "the accumulator is bounded"
        );
        // An independent statement of the ranking: cosine descending, then
        // left position, then right position.
        let mut every_pair = Vec::new();
        for (i, l) in left.chunks.values().enumerate() {
            for (j, r) in right.chunks.values().enumerate() {
                let dot: f32 = l.vector.iter().zip(&r.vector).map(|(a, b)| a * b).sum();
                let norm = l.vector.iter().map(|v| v * v).sum::<f32>().sqrt()
                    * r.vector.iter().map(|v| v * v).sum::<f32>().sqrt();
                every_pair.push(((dot / norm).clamp(-1.0, 1.0), i, j));
            }
        }
        every_pair.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let expected: Vec<(u32, u32, u32)> = every_pair
            .iter()
            .filter(|p| p.0 >= MIN_SIMILARITY_COSINE)
            .take(MAX_EVIDENCE_PER_SIDE * 2)
            .map(|p| (p.0.to_bits(), p.1 as u32, p.2 as u32))
            .collect();
        let actual: Vec<(u32, u32, u32)> = whole
            .top_pairs
            .iter()
            .map(|p| (p.cosine_bits, p.left, p.right))
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(
            whole.best.map(|b| (b.cosine_bits, b.left, b.right)),
            Some((
                every_pair[0].0.to_bits(),
                every_pair[0].1 as u32,
                every_pair[0].2 as u32
            ))
        );
        for (rows, columns) in [(32, 64), (5, 7), (1, 1), (70, 1), (1, 130)] {
            for persist in [false, true] {
                let (tiled, comparisons) = run_pair(&left, &right, rows, columns, persist);
                assert_eq!(comparisons, 70 * 130);
                assert_eq!(tiled, whole, "tile {rows}x{columns}, persisted={persist}");
                assert_eq!(
                    finish(&left, &right, &tiled)
                        .iter()
                        .map(|e| (
                            e.kind,
                            e.discovery_cosine.to_bits(),
                            e.source_evidence.clone(),
                            e.target_evidence.clone()
                        ))
                        .collect::<Vec<_>>(),
                    finish(&left, &right, &whole)
                        .iter()
                        .map(|e| (
                            e.kind,
                            e.discovery_cosine.to_bits(),
                            e.source_evidence.clone(),
                            e.target_evidence.clone()
                        ))
                        .collect::<Vec<_>>(),
                );
            }
        }
    }

    #[test]
    fn a_tile_is_bounded_and_cancellation_leaves_no_half_state_for_the_caller_to_keep() {
        let left = big_side("left", MAX_TILE_COMPARISONS + 1, 1);
        let right = big_side("right", 2, 2);
        let rows: Vec<_> = left.chunks.values().cloned().collect();
        let columns: Vec<_> = right.chunks.values().cloned().collect();
        let error =
            process_tile(&mut PairAccumulator::default(), &rows, 0, &columns, 0, None).unwrap_err();
        assert!(error.to_string().contains("tile is limited"));

        let flag = AtomicBool::new(true);
        let error = process_tile(
            &mut PairAccumulator::default(),
            &rows[..2],
            0,
            &columns,
            0,
            Some(&flag),
        )
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
    }

    #[test]
    fn mismatched_or_non_finite_vectors_are_rejected() {
        let a = vec![chunk("a", "sha256:a", "a", vec![1.0, 0.0])];
        let b = vec![chunk("b", "sha256:b", "b", vec![1.0, 0.0, 0.0])];
        assert!(process_tile(&mut PairAccumulator::default(), &a, 0, &b, 0, None).is_err());
        let c = vec![chunk("c", "sha256:c", "c", vec![f32::NAN, 0.0])];
        assert!(process_tile(&mut PairAccumulator::default(), &a, 0, &c, 0, None).is_err());
    }

    #[test]
    fn finishing_needs_the_referenced_chunks() {
        let a = side(
            "a",
            vec![chunk(
                "a",
                "sha256:a",
                "Project deadline October 20.",
                vec![1.0, 0.0],
            )],
        );
        let b = side(
            "b",
            vec![chunk(
                "b",
                "sha256:b",
                "Huling araw October 20.",
                vec![1.0, 0.0],
            )],
        );
        let (accumulator, _) = run_pair(&a, &b, 8, 8, false);
        let (left_positions, right_positions) = referenced_positions(&accumulator);
        assert_eq!((left_positions, right_positions), (vec![0], vec![0]));
        let empty = PairSide {
            chunks: BTreeMap::new(),
            ..a.clone()
        };
        assert!(finish_pair(&empty, &b, &accumulator, "space").is_err());
    }
}
