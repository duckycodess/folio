use crate::contracts::DocumentRecord;
use crate::error::{CoreError, CoreResult};
use sha2::{Digest, Sha256};

pub const INTERIM_CHUNKER_VERSION: &str = "paragraph-800-utf16-v1";
pub const DEFAULT_MAX_CHUNK_UTF16: usize = 800;

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub document_id: String,
    pub ordinal: usize,
    pub text: String,
    pub start: u32,
    pub end: u32,
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
        record.content_hash = Some(sha256(&content));
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
    max_chunk_utf16: usize,
}

impl InterimTextChunker {
    pub fn new(documents: Vec<TextDocument>) -> Self {
        Self {
            documents,
            max_chunk_utf16: DEFAULT_MAX_CHUNK_UTF16,
        }
    }

    pub fn with_max_chunk_utf16(mut self, max_chunk_utf16: usize) -> CoreResult<Self> {
        if max_chunk_utf16 == 0 {
            return Err(CoreError::Message("Chunk size must be positive.".into()));
        }
        self.max_chunk_utf16 = max_chunk_utf16;
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
            self.max_chunk_utf16,
            document.record.content_hash.as_deref().unwrap_or_default(),
        )
    }
}

pub fn chunk_text(
    document_id: &str,
    content: &str,
    max_chunk_utf16: usize,
    content_hash: &str,
) -> CoreResult<Vec<Chunk>> {
    if max_chunk_utf16 == 0 {
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
            paragraphs.extend(split_segment(content, start, end, max_chunk_utf16));
        }
        start = end;
    }
    if start < content.len() {
        paragraphs.extend(split_segment(
            content,
            start,
            content.len(),
            max_chunk_utf16,
        ));
    }

    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in paragraphs {
        if let Some((previous_start, previous_end)) = merged.last_mut() {
            if utf16_len(&content[*previous_start..end]) <= max_chunk_utf16 {
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
            start: byte_to_utf16(content, start),
            end: byte_to_utf16(content, end),
            content_hash: content_hash.into(),
        })
        .collect())
}

fn split_segment(content: &str, start: usize, end: usize, max_utf16: usize) -> Vec<(usize, usize)> {
    if utf16_len(&content[start..end]) <= max_utf16 {
        return vec![(start, end)];
    }
    let mut pieces = Vec::new();
    let mut piece_start = start;
    let mut piece_len = 0;
    for (relative, character) in content[start..end].char_indices() {
        let absolute = start + relative;
        let character_len = character.len_utf16();
        if piece_len > 0 && piece_len + character_len > max_utf16 {
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

pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

pub fn byte_to_utf16(text: &str, byte_offset: usize) -> u32 {
    text[..byte_offset].encode_utf16().count() as u32
}

pub fn utf16_slice(text: &str, start: u32, end: u32) -> Option<String> {
    let mut byte_start = None;
    let mut byte_end = None;
    let mut units = 0_u32;
    for (byte, character) in text.char_indices() {
        if units == start {
            byte_start = Some(byte);
        }
        units += character.len_utf16() as u32;
        if units == end {
            byte_end = Some(byte + character.len_utf8());
            break;
        }
    }
    if start == end {
        return (utf16_len(text) as u32 >= start).then_some(String::new());
    }
    match (byte_start, byte_end) {
        (Some(start), Some(end)) => Some(text[start..end].into()),
        _ => None,
    }
}

pub fn sha256(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{DocumentRecord, Language};

    fn record(id: &str) -> DocumentRecord {
        DocumentRecord {
            id: id.into(),
            relative_path: id.into(),
            name: id.into(),
            title: id.into(),
            language: Language::Mixed,
            size_bytes: 0,
            content: None,
            content_hash: None,
        }
    }

    #[test]
    fn utf16_offsets_slice_non_ascii_text_exactly() {
        let content = "Pagsasanay — ñ … 📄 deadline";
        let chunks = chunk_text("notes.md", content, 800, &sha256(content)).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            utf16_slice(content, chunks[0].start, chunks[0].end).unwrap(),
            content
        );
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks[0].end as usize, utf16_len(content));
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
    }
}
