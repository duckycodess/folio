use crate::contracts::DocumentRecord;
use crate::error::{CoreError, CoreResult};
use sha2::{Digest, Sha256};

pub const INTERIM_CHUNKER_VERSION: &str = "paragraph-800-utf8-v2";
pub const DEFAULT_MAX_CHUNK_BYTES: usize = 800;

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub document_id: String,
    pub ordinal: usize,
    pub text: String,
    /// UTF-8 byte offsets into the source document, on character boundaries.
    pub start: usize,
    pub end: usize,
    pub content_hash: String,
}

#[derive(Clone, Debug)]
pub struct TextDocument {
    pub record: DocumentRecord,
    pub content: String,
}

impl TextDocument {
    pub fn new(mut record: DocumentRecord, content: impl Into<String>) -> Self {
        let content = content.into();
        if record.content_hash.is_none() {
            record.content_hash = Some(content_hash(&content));
        }
        record.size_bytes = content.len() as u64;
        Self { record, content }
    }
}

/// Interim until #3 supplies persisted chunks from its indexer.
pub trait ChunkSource {
    fn documents(&self) -> Vec<DocumentRecord>;
    fn chunks(&self, document_id: &str) -> CoreResult<Vec<Chunk>>;
}

#[derive(Clone, Debug)]
pub struct InterimTextChunker {
    documents: Vec<TextDocument>,
    max_chunk_bytes: usize,
}

impl InterimTextChunker {
    pub fn new(documents: Vec<TextDocument>) -> Self {
        Self {
            documents,
            max_chunk_bytes: DEFAULT_MAX_CHUNK_BYTES,
        }
    }

    pub fn with_max_chunk_bytes(mut self, max_chunk_bytes: usize) -> CoreResult<Self> {
        if max_chunk_bytes == 0 {
            return Err(CoreError::Message("Chunk size must be positive.".into()));
        }
        self.max_chunk_bytes = max_chunk_bytes;
        Ok(self)
    }

    pub fn all_chunks(&self) -> CoreResult<Vec<Chunk>> {
        self.documents
            .iter()
            .try_fold(Vec::new(), |mut all, document| {
                all.extend(self.chunks(&document.record.id)?);
                Ok(all)
            })
    }
}

impl ChunkSource for InterimTextChunker {
    fn documents(&self) -> Vec<DocumentRecord> {
        self.documents
            .iter()
            .map(|document| document.record.clone())
            .collect()
    }

    fn chunks(&self, document_id: &str) -> CoreResult<Vec<Chunk>> {
        let document = self
            .documents
            .iter()
            .find(|document| document.record.id == document_id)
            .ok_or_else(|| CoreError::Message(format!("Unknown document: {document_id}")))?;
        chunk_text(
            document_id,
            &document.content,
            self.max_chunk_bytes,
            document.record.content_hash.as_deref().unwrap_or_default(),
        )
    }
}

pub fn chunk_text(
    document_id: &str,
    content: &str,
    max_chunk_bytes: usize,
    content_hash: &str,
) -> CoreResult<Vec<Chunk>> {
    if max_chunk_bytes == 0 {
        return Err(CoreError::Message("Chunk size must be positive.".into()));
    }
    if content.is_empty() {
        return Ok(Vec::new());
    }
    let mut paragraphs = Vec::new();
    let mut start = 0;
    for (separator, _) in content.match_indices("\n\n") {
        let end = separator + 2;
        if start < end {
            paragraphs.extend(split_segment(content, start, end, max_chunk_bytes));
        }
        start = end;
    }
    if start < content.len() {
        paragraphs.extend(split_segment(
            content,
            start,
            content.len(),
            max_chunk_bytes,
        ));
    }

    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in paragraphs {
        if let Some((previous_start, previous_end)) = merged.last_mut() {
            if content[*previous_start..end].len() <= max_chunk_bytes {
                *previous_end = end;
                continue;
            }
        }
        merged.push((start, end));
    }

    Ok(merged
        .into_iter()
        .enumerate()
        .map(|(ordinal, (start, end))| Chunk {
            document_id: document_id.into(),
            ordinal,
            text: content[start..end].to_owned(),
            start,
            end,
            content_hash: content_hash.into(),
        })
        .collect())
}

fn split_segment(content: &str, start: usize, end: usize, max_bytes: usize) -> Vec<(usize, usize)> {
    if content[start..end].len() <= max_bytes {
        return vec![(start, end)];
    }
    let mut pieces = Vec::new();
    let mut piece_start = start;
    let mut piece_len = 0_usize;
    for (relative, character) in content[start..end].char_indices() {
        let absolute = start + relative;
        let character_len = character.len_utf8();
        if piece_len > 0 && piece_len + character_len > max_bytes {
            pieces.push((piece_start, absolute));
            piece_start = absolute;
            piece_len = 0;
        }
        piece_len += character_len;
    }
    if piece_start < end {
        pieces.push((piece_start, end));
    }
    pieces
}

pub fn content_hash(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{DocumentRecord, Language};
    use serde_json::Value;
    use std::path::PathBuf;

    fn record(id: &str) -> DocumentRecord {
        DocumentRecord {
            id: id.into(),
            workspace_id: "test-workspace".into(),
            relative_path: id.into(),
            name: id.into(),
            title: id.into(),
            language: Language::Mixed,
            media_type: "text/markdown".into(),
            size_bytes: 0,
            modified_at_ms: None,
            content: None,
            content_hash: None,
        }
    }

    #[test]
    fn utf8_offsets_slice_non_ascii_text_exactly() {
        let content = "Pagsasanay — ñ … 📄 deadline";
        let chunks = chunk_text("notes.md", content, 800, &content_hash(content)).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(&content[chunks[0].start..chunks[0].end], content);
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks[0].end, content.len());
        assert_eq!(chunks[0].content_hash, content_hash(content));
    }

    #[test]
    fn utf8_offsets_keep_non_ascii_prefix_bytes() {
        let content = "ñ\n\nOctober 20";
        let chunks = chunk_text("notes.md", content, 10, &content_hash(content)).unwrap();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks[0].end, 4);
        assert_eq!(chunks[0].text, "ñ\n\n");
        assert_eq!(chunks[1].start, 4);
        assert_eq!(chunks[1].end, 14);
        assert_eq!(chunks[1].text, "October 20");
        assert_eq!(&content[chunks[1].start..chunks[1].end], chunks[1].text);
    }

    #[test]
    fn paragraph_chunks_merge_without_splitting_surrogates() {
        let content = "one\n\ntwo 📄\n\nthree";
        let source = InterimTextChunker::new(vec![TextDocument::new(record("notes.md"), content)]);
        let chunks = source.chunks("notes.md").unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, content);
    }

    #[test]
    fn content_hash_changes_when_a_chunk_source_changes() {
        let first = TextDocument::new(record("notes.md"), "first");
        let second = TextDocument::new(record("notes.md"), "second");
        assert_ne!(first.record.content_hash, second.record.content_hash);
        assert!(first
            .record
            .content_hash
            .as_deref()
            .is_some_and(|hash| hash.starts_with("sha256:")));
    }

    #[test]
    fn contract_fixture_offsets_and_hashes_use_utf8_bytes() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/contracts/contract-cases.json");
        let cases: Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("contract fixture exists"))
                .expect("contract fixture is valid JSON");

        for case in cases["offsets"]["cases"].as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            let start = case["passage"]["start"].as_u64().unwrap() as usize;
            let end = case["passage"]["end"].as_u64().unwrap() as usize;
            let expected_hash = case["contentHash"].as_str().unwrap();
            assert_eq!(content_hash(text), expected_hash);
            assert!(text.is_char_boundary(start));
            assert!(text.is_char_boundary(end));
            assert_eq!(&text[start..end], case["passage"]["text"].as_str().unwrap());

            let chunks = chunk_text("fixture:case", &text[start..end], usize::MAX, expected_hash)
                .expect("fixture passage chunks");
            assert_eq!(chunks.len(), 1);
            assert_eq!(chunks[0].start, 0);
            assert_eq!(chunks[0].end, end - start);
            assert_eq!(chunks[0].text, case["passage"]["text"].as_str().unwrap());
            assert_eq!(chunks[0].content_hash, expected_hash);
            let passage = crate::grounding::passages_from_chunks(&chunks)
                .into_iter()
                .next()
                .unwrap();
            assert_eq!(passage.document_content_hash, expected_hash);
            assert_eq!(passage.offset_unit, crate::contracts::OffsetUnit::Utf8Byte);
        }

        for hash in cases["hashes"].as_array().unwrap() {
            assert_eq!(
                content_hash(hash["text"].as_str().unwrap()),
                hash["expected"].as_str().unwrap()
            );
        }
    }
}
