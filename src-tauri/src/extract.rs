use std::ops::Range;
use std::path::Path;
use crate::db::NativeResult;
use crate::error::{error, ErrorCode};
use crate::identity::media_type_for_path;

pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_PDF_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PDF_PAGE_CONTENT: usize = 16 * 1024 * 1024;
const MAX_CHUNK_CHARS: usize = 1200;
const PAGE_SEPARATOR: &str = "\n\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Text,
    Markdown,
    Pdf,
}

impl MediaKind {
    pub fn from_path(path: &Path) -> Option<MediaKind> {
        match media_type_for_path(path.file_name()?.to_str()?)? {
            "text/plain" => Some(MediaKind::Text),
            "text/markdown" => Some(MediaKind::Markdown),
            "application/pdf" => Some(MediaKind::Pdf),
            _ => None,
        }
    }

    pub fn media_type(self) -> &'static str {
        match self {
            MediaKind::Text => "text/plain",
            MediaKind::Markdown => "text/markdown",
            MediaKind::Pdf => "application/pdf",
        }
    }

    pub fn max_bytes(self) -> u64 {
        if self == MediaKind::Pdf { MAX_PDF_BYTES } else { MAX_TEXT_BYTES }
    }
}

/// The document's extracted text. PDF pages are joined with a blank line; `pages`
/// holds each page's byte range in `text`. Plain text has a single unpaged segment.
pub struct ExtractedText {
    pub text: String,
    pub pages: Vec<(Option<u32>, Range<usize>)>,
}

pub enum Extraction {
    Text(ExtractedText),
    /// Readable file that Folio cannot index, with the reason shown to the user.
    Unsupported(String),
}

#[derive(Debug, PartialEq)]
pub struct Chunk {
    pub ordinal: usize,
    pub text: String,
    /// UTF-8 byte offsets into `ExtractedText::text` (the frozen `utf8Byte` unit).
    pub start: usize,
    pub end: usize,
    pub page: Option<u32>,
}

pub fn extract(kind: MediaKind, bytes: &[u8]) -> NativeResult<Extraction> {
    if bytes.len() as u64 > kind.max_bytes() {
        return Ok(Extraction::Unsupported(format!("Larger than the {} MiB limit for this file type.", kind.max_bytes() / 1024 / 1024)));
    }
    match kind {
        MediaKind::Text | MediaKind::Markdown => {
            let text = decode_text(bytes)?;
            let len = text.len();
            Ok(Extraction::Text(ExtractedText { text, pages: vec![(None, 0..len)] }))
        }
        MediaKind::Pdf => extract_pdf(bytes),
    }
}

/// Text shown by the reader. Offsets in search results and evidence index into this string.
pub fn document_text(kind: MediaKind, bytes: &[u8]) -> NativeResult<String> {
    match extract(kind, bytes)? {
        Extraction::Text(extracted) => Ok(extracted.text),
        Extraction::Unsupported(reason) => Err(error(ErrorCode::DocumentNotText, reason)),
    }
}

/// The decoded text exactly as stored (a byte-order mark is kept), so offsets match `read_document`.
fn decode_text(bytes: &[u8]) -> NativeResult<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| error(ErrorCode::DocumentNotText, "This document is not valid UTF-8 text."))
}

fn extract_pdf(bytes: &[u8]) -> NativeResult<Extraction> {
    let owned = bytes.to_vec();
    let result = std::panic::catch_unwind(move || -> Result<Vec<(u32, String)>, String> {
        let document = lopdf::Document::load_mem(&owned).map_err(|error| error.to_string())?;
        if document.is_encrypted() {
            return Err("The PDF is encrypted.".into());
        }
        let mut pages = Vec::new();
        for number in document.get_pages().keys() {
            let text = document.extract_text_with_limit(&[*number], MAX_PDF_PAGE_CONTENT).unwrap_or_default();
            pages.push((*number, text));
        }
        Ok(pages)
    });
    let pages = match result {
        Ok(Ok(pages)) => pages,
        Ok(Err(reason)) => return Err(error(ErrorCode::DocumentNotText, "The PDF could not be read.").with_detail("cause", reason)),
        Err(_) => return Err(error(ErrorCode::DocumentNotText, "The PDF could not be read.")),
    };
    if pages.iter().all(|(_, text)| text.trim().is_empty()) {
        return Ok(Extraction::Unsupported("No text layer was found. Scanned PDFs need OCR, which Folio does not support.".into()));
    }
    let mut text = String::new();
    let mut ranges = Vec::new();
    for (index, (number, page_text)) in pages.iter().enumerate() {
        if index > 0 { text.push_str(PAGE_SEPARATOR); }
        let start = text.len();
        text.push_str(page_text.trim_end());
        ranges.push((Some(*number), start..text.len()));
    }
    Ok(Extraction::Text(ExtractedText { text, pages: ranges }))
}

/// The line containing `start..end`, without its line ending (LF or CRLF) or
/// surrounding whitespace. Used for Ripple passages and edit excerpts.
pub fn line_bounds(text: &str, start: usize, end: usize) -> (usize, usize) {
    let from = text[..start].rfind('\n').map_or(0, |newline| newline + 1);
    let to = text[end..].find('\n').map_or(text.len(), |newline| end + newline);
    let line = &text[from..to];
    (from + (line.len() - line.trim_start().len()), to - (line.len() - line.trim_end().len()))
}

/// First Markdown H1, else the first short non-empty line.
pub fn title_of(text: &str) -> Option<String> {
    let heading = text.lines().find_map(|line| line.strip_prefix("# ").map(str::trim));
    heading
        .or_else(|| text.lines().map(str::trim).find(|line| !line.is_empty() && line.chars().count() <= 120))
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
}

/// Paragraph chunks of at most ~MAX_CHUNK_CHARS that never cross a page or split a Markdown link.
pub fn chunk(extracted: &ExtractedText) -> Vec<Chunk> {
    let text = extracted.text.as_str();
    let mut ranges: Vec<(Option<u32>, Range<usize>)> = Vec::new();
    for (page, segment) in &extracted.pages {
        let mut current: Option<Range<usize>> = None;
        for paragraph in paragraphs(text, segment.clone()) {
            for piece in split_long(text, paragraph) {
                current = match current {
                    Some(open) if char_len(text, open.start..piece.end) <= MAX_CHUNK_CHARS => Some(open.start..piece.end),
                    Some(open) => {
                        ranges.push((*page, open));
                        Some(piece)
                    }
                    None => Some(piece),
                };
            }
        }
        if let Some(open) = current { ranges.push((*page, open)); }
    }
    ranges
        .into_iter()
        .enumerate()
        .map(|(ordinal, (page, range))| Chunk { ordinal, text: text[range.clone()].to_owned(), start: range.start, end: range.end, page })
        .collect()
}

fn char_len(text: &str, range: Range<usize>) -> usize {
    text[range].chars().count()
}

/// Trimmed paragraph ranges separated by blank lines.
fn paragraphs(text: &str, segment: Range<usize>) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut paragraph_start: Option<usize> = None;
    let mut line_start = segment.start;
    for line in text[segment.clone()].split_inclusive('\n') {
        let start = line_start;
        line_start += line.len();
        if line.trim().is_empty() {
            if let Some(open) = paragraph_start.take() { push_trimmed(text, open..start, &mut result); }
        } else if paragraph_start.is_none() {
            paragraph_start = Some(start);
        }
    }
    if let Some(open) = paragraph_start { push_trimmed(text, open..segment.end, &mut result); }
    result
}

fn push_trimmed(text: &str, range: Range<usize>, out: &mut Vec<Range<usize>>) {
    let slice = &text[range.clone()];
    let leading = slice.len() - slice.trim_start().len();
    let trailing = slice.len() - slice.trim_end().len();
    if leading + trailing < slice.len() {
        out.push(range.start + leading..range.end - trailing);
    }
}

/// Splits a paragraph longer than the chunk limit at whitespace outside Markdown links.
fn split_long(text: &str, paragraph: Range<usize>) -> Vec<Range<usize>> {
    if char_len(text, paragraph.clone()) <= MAX_CHUNK_CHARS {
        return vec![paragraph];
    }
    let links = markdown_links(&text[paragraph.clone()])
        .into_iter()
        .map(|link| paragraph.start + link.whole.start..paragraph.start + link.whole.end)
        .collect::<Vec<_>>();
    let inside_link = |index: usize| links.iter().any(|link| index > link.start && index < link.end);
    let mut pieces = Vec::new();
    let mut start = paragraph.start;
    while char_len(text, start..paragraph.end) > MAX_CHUNK_CHARS {
        let limit = text[start..].char_indices().nth(MAX_CHUNK_CHARS).map(|(offset, _)| start + offset).unwrap_or(paragraph.end);
        let cut = text[start..limit]
            .char_indices()
            .rev()
            .map(|(offset, ch)| (start + offset, ch))
            .find(|(index, ch)| ch.is_whitespace() && *index > start && !inside_link(*index))
            .map(|(index, _)| index)
            .unwrap_or(limit);
        push_trimmed(text, start..cut, &mut pieces);
        start = cut;
    }
    push_trimmed(text, start..paragraph.end, &mut pieces);
    pieces
}

pub struct MarkdownLink {
    /// Byte range of `[label](target)`.
    pub whole: Range<usize>,
    pub target: String,
}

/// `[label](target)` with non-empty label and target, matching the frontend's discovery rule.
pub fn markdown_links(text: &str) -> Vec<MarkdownLink> {
    let bytes = text.as_bytes();
    let mut links = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let Some(close) = text[index + 1..].find(']').map(|offset| index + 1 + offset) else { break };
        if close == index + 1 || bytes.get(close + 1) != Some(&b'(') {
            index += 1;
            continue;
        }
        let target_start = close + 2;
        match text[target_start..].find(')').map(|offset| target_start + offset) {
            Some(end) if end > target_start => {
                links.push(MarkdownLink { whole: index..end + 1, target: text[target_start..end].to_owned() });
                index = end + 1;
            }
            _ => index += 1,
        }
    }
    links
}

#[cfg(test)]
pub mod testpdf {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};

    fn finish(mut doc: Document, pages_id: lopdf::ObjectId, kids: Vec<Object>, resources_id: lopdf::ObjectId) -> Vec<u8> {
        let count = kids.len() as i64;
        doc.objects.insert(pages_id, Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => count, "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }));
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    /// A text-layer PDF with one page per entry; each page's lines are drawn separately.
    pub fn text_pdf(pages: &[&[&str]]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let mut kids = Vec::new();
        for lines in pages {
            let mut operations = vec![Operation::new("BT", vec![]), Operation::new("Tf", vec!["F1".into(), 12.into()]), Operation::new("Td", vec![72.into(), 760.into()])];
            for line in *lines {
                operations.push(Operation::new("Tj", vec![Object::string_literal(*line)]));
                operations.push(Operation::new("Td", vec![0.into(), (-18).into()]));
            }
            operations.push(Operation::new("ET", vec![]));
            let content_id = doc.add_object(Stream::new(dictionary! {}, Content { operations }.encode().unwrap()));
            kids.push(doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id }).into());
        }
        finish(doc, pages_id, kids, resources_id)
    }

    /// An image-only page with no text layer, standing in for a scanned document.
    pub fn image_only_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let image_id = doc.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 2, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 },
            vec![0, 255, 255, 0],
        ));
        let resources_id = doc.add_object(dictionary! { "XObject" => dictionary! { "Im1" => image_id } });
        let operations = vec![
            Operation::new("q", vec![]),
            Operation::new("cm", vec![400.into(), 0.into(), 0.into(), 400.into(), 100.into(), 300.into()]),
            Operation::new("Do", vec!["Im1".into()]),
            Operation::new("Q", vec![]),
        ];
        let content_id = doc.add_object(Stream::new(dictionary! {}, Content { operations }.encode().unwrap()));
        let kids = vec![doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id }).into()];
        finish(doc, pages_id, kids, resources_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> ExtractedText {
        ExtractedText { text: text.to_owned(), pages: vec![(None, 0..text.len())] }
    }

    #[test]
    fn chunk_offsets_are_utf8_bytes_on_character_boundaries() {
        let text = "  Community Learning Project — due October 20.\n\nAng huling araw ay 😀 October 20.\n";
        let chunks = chunk(&plain(text));
        assert_eq!(chunks.len(), 1);
        let chunk = &chunks[0];
        assert!(text.is_char_boundary(chunk.start) && text.is_char_boundary(chunk.end));
        assert_eq!(&text.as_bytes()[chunk.start..chunk.end], chunk.text.as_bytes());
        assert_eq!(chunk.start, 2);
    }

    #[test]
    fn long_text_splits_into_bounded_chunks_without_breaking_links() {
        let sentence = "Kailangan ng consent form bago ang interview session. ";
        let mut text = sentence.repeat(30);
        text.push_str("Basahin ang [methodology notes](methodology-notes.md) para sa detalye. ");
        text.push_str(&sentence.repeat(30));
        let chunks = chunk(&plain(&text));
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.text.chars().count() <= MAX_CHUNK_CHARS);
            let opens = chunk.text.matches("[methodology").count();
            let closes = chunk.text.matches("notes.md)").count();
            assert_eq!(opens, closes, "a link was split across chunks");
        }
        assert_eq!(chunks.iter().filter(|chunk| chunk.text.contains("[methodology notes](methodology-notes.md)")).count(), 1);
    }

    #[test]
    fn small_paragraphs_merge_and_blank_lines_separate() {
        let chunks = chunk(&plain("# Title\n\n\nFirst.\r\n\r\nSecond line.\n"));
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("# Title") && chunks[0].text.ends_with("Second line."));
    }

    #[test]
    fn text_pdf_is_extracted_per_page() {
        let bytes = testpdf::text_pdf(&[&["Consent guide page one."], &["Pirma bago ang interview."]]);
        let Extraction::Text(extracted) = extract(MediaKind::Pdf, &bytes).unwrap() else { panic!("expected text") };
        assert_eq!(extracted.pages.len(), 2);
        let chunks = chunk(&extracted);
        assert_eq!(chunks.iter().map(|chunk| chunk.page).collect::<Vec<_>>(), vec![Some(1), Some(2)]);
        assert!(chunks[0].text.contains("Consent guide page one."));
        assert!(chunks[1].text.contains("Pirma bago ang interview."));
    }

    #[test]
    fn image_only_pdf_is_unsupported_not_failed() {
        assert!(matches!(extract(MediaKind::Pdf, &testpdf::image_only_pdf()).unwrap(), Extraction::Unsupported(reason) if reason.contains("OCR")));
    }

    #[test]
    fn a_byte_order_mark_is_kept_so_offsets_match_the_reader() {
        let Extraction::Text(extracted) = extract(MediaKind::Markdown, "\u{feff}# Tala".as_bytes()).unwrap() else { panic!("expected text") };
        let chunks = chunk(&extracted);
        assert_eq!(&extracted.text[chunks[0].start..chunks[0].end], "\u{feff}# Tala");
    }

    #[test]
    fn corrupt_pdf_and_invalid_utf8_are_errors() {
        assert!(extract(MediaKind::Pdf, b"%PDF-1.5 broken").is_err());
        assert!(extract(MediaKind::Text, &[0xff, 0xfe, 0x00]).is_err());
    }

    const CONSENT_GUIDE: &[&[&str]] = &[
        &[
            "Interview Consent Form Guide",
            "Language: Taglish",
            "Bago ang interview, kailangan ng signed consent form ng bawat participant.",
            "Ipaliwanag ang purpose ng research at ang karapatang umatras anumang oras.",
        ],
        &["Store signed forms in the research folder. Huwag i-upload sa public drive.", "Related: methodology notes and review reminders."],
    ];

    fn fixtures() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
    }

    /// Regenerates the committed PDF fixtures: `cargo test -- --ignored write_pdf_fixtures`.
    #[test]
    #[ignore]
    fn write_pdf_fixtures() {
        std::fs::write(fixtures().join("documents/research/consent-form-guide.pdf"), testpdf::text_pdf(CONSENT_GUIDE)).unwrap();
        std::fs::create_dir_all(fixtures().join("scanned")).unwrap();
        std::fs::write(fixtures().join("scanned/scanned-sample.pdf"), testpdf::image_only_pdf()).unwrap();
    }

    #[test]
    fn committed_pdf_fixtures_extract_as_documented() {
        let guide = std::fs::read(fixtures().join("documents/research/consent-form-guide.pdf")).unwrap();
        let Extraction::Text(extracted) = extract(MediaKind::Pdf, &guide).unwrap() else { panic!("expected a text layer") };
        assert!(extracted.text.contains("karapatang umatras"));
        assert_eq!(extracted.pages.len(), 2);
        let scanned = std::fs::read(fixtures().join("scanned/scanned-sample.pdf")).unwrap();
        assert!(matches!(extract(MediaKind::Pdf, &scanned).unwrap(), Extraction::Unsupported(_)));
    }

    #[test]
    fn markdown_links_match_frontend_rule() {
        let links = markdown_links("See [plan](../projects/project-plan.md) and [](empty) and [x]() end.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "../projects/project-plan.md");
    }
}
