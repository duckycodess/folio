use std::ops::Range;
use std::path::Path;
use crate::db::NativeResult;
use crate::error::{error, ErrorCode};
use crate::identity::media_type_for_path;

/// Bump when extraction changes in a way that could read a previously failing file, so
/// documents that failed under an older version are retried at once (ADR 0009).
pub const EXTRACTOR_VERSION: u32 = 1;
pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_PDF_BYTES: u64 = 20 * 1024 * 1024;
/// Bounds every decompressed PDF stream: object and cross-reference streams while
/// loading, and each page's content while extracting. Exceeding it is an error, not
/// an allocation, so a decompression bomb cannot exhaust memory.
const MAX_PDF_STREAM_BYTES: usize = 16 * 1024 * 1024;
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
    /// PDF pages whose text could not be extracted; the rest was indexed.
    pub skipped_pages: Vec<u32>,
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
            Ok(Extraction::Text(ExtractedText { text, pages: vec![(None, 0..len)], skipped_pages: Vec::new() }))
        }
        MediaKind::Pdf => extract_pdf(bytes, MAX_PDF_STREAM_BYTES),
    }
}

/// Text shown by the reader, with each PDF page's byte range and the pages that couldn't be
/// read. Offsets in search results and evidence index into `text`.
pub fn document_pages(kind: MediaKind, bytes: &[u8]) -> NativeResult<ExtractedText> {
    match extract(kind, bytes)? {
        Extraction::Text(extracted) => Ok(extracted),
        Extraction::Unsupported(reason) => Err(error(ErrorCode::DocumentNotText, reason)),
    }
}

/// The decoded text exactly as stored (a byte-order mark is kept), so offsets match `read_document`.
fn decode_text(bytes: &[u8]) -> NativeResult<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| error(ErrorCode::DocumentNotText, "This document is not valid UTF-8 text."))
}

/// Loads a PDF with every object and cross-reference stream bounded by `stream_limit`.
/// lopdf drops an object stream that would exceed it rather than allocating it.
fn load_pdf(bytes: &[u8], stream_limit: usize) -> Result<lopdf::Document, String> {
    lopdf::Document::load_mem_with_options(bytes, lopdf::LoadOptions::with_max_decompressed_size(stream_limit)).map_err(|error| error.to_string())
}

fn extract_pdf(bytes: &[u8], stream_limit: usize) -> NativeResult<Extraction> {
    let owned = bytes.to_vec();
    let result = std::panic::catch_unwind(move || -> Result<Vec<(u32, Result<String, String>)>, String> {
        let document = load_pdf(&owned, stream_limit)?;
        if document.is_encrypted() {
            return Err("The PDF is encrypted.".into());
        }
        Ok(document
            .get_pages()
            .keys()
            .map(|number| (*number, document.extract_text_with_limit(&[*number], stream_limit).map_err(|error| error.to_string())))
            .collect())
    });
    let pages = match result {
        Ok(Ok(pages)) => pages,
        Ok(Err(reason)) => return Err(error(ErrorCode::DocumentNotText, "The PDF could not be read.").with_detail("cause", reason)),
        Err(_) => return Err(error(ErrorCode::DocumentNotText, "The PDF could not be read.")),
    };
    if pages.is_empty() {
        return Err(error(ErrorCode::DocumentNotText, "The PDF has no pages Folio could read."));
    }
    let first_failure = pages.iter().find_map(|(_, page)| page.as_ref().err().cloned());
    let had_text = pages.iter().any(|(_, page)| page.as_ref().is_ok_and(|text| !text.trim().is_empty()));
    let pages: Vec<(u32, Result<String, String>, bool)> = pages
        .into_iter()
        .map(|(number, page)| {
            let blank = page.as_ref().is_ok_and(|text| text.trim().is_empty());
            (number, page.map(|text| readable_lines(&text)), blank)
        })
        .collect();
    let has_text = pages.iter().any(|(_, page, _)| page.as_ref().is_ok_and(|text| !text.trim().is_empty()));
    if !has_text {
        // A page that failed is not evidence of a scan: only report OCR when every page
        // was read and none had text.
        return match first_failure {
            Some(cause) => Err(error(ErrorCode::DocumentNotText, "The PDF's text could not be extracted.").with_detail("cause", cause)),
            None if had_text => Ok(Extraction::Unsupported("The PDF's text layer is unreadable: its fonts do not map to real characters.".into())),
            None => Ok(Extraction::Unsupported("No text layer was found. Scanned PDFs need OCR, which Folio does not support.".into())),
        };
    }
    let mut text = String::new();
    let mut ranges = Vec::new();
    let mut skipped_pages = Vec::new();
    for (number, page, blank) in &pages {
        // A page whose text was all unreadable counts as not read; a blank page does not.
        let Some(page_text) = page.as_ref().ok().filter(|text| *blank || !text.trim().is_empty()) else {
            skipped_pages.push(*number);
            continue;
        };
        if !ranges.is_empty() { text.push_str(PAGE_SEPARATOR); }
        let start = text.len();
        text.push_str(page_text.trim_end());
        ranges.push((Some(*number), start..text.len()));
    }
    Ok(Extraction::Text(ExtractedText { text, pages: ranges, skipped_pages }))
}

/// Punctuation that ordinary text uses; anything else that is neither a letter, digit
/// nor whitespace counts against a line's readability.
const COMMON_PUNCTUATION: &str = ".,;:!?'\"()[]{}-–—/\\&%@#*+=<>|~^$€£¥₱°•·…‘’“”«»_";

/// Whether a PDF text line reads as language rather than glyph codes. A PDF whose fonts
/// lack a Unicode mapping yields lines such as `&T˛˛˛m)7Wk˛7B_)Z˛#W)7`, which would
/// otherwise be indexed, embedded and shown as connection evidence.
fn is_readable_line(line: &str) -> bool {
    let visible: Vec<char> = line.chars().filter(|ch| !ch.is_whitespace()).collect();
    if visible.is_empty() { return true; }
    let odd = visible.iter().filter(|ch| !ch.is_alphanumeric() && !COMMON_PUNCTUATION.contains(**ch)).count();
    let odd_limit = if visible.len() < 4 { 0.5 } else { 0.25 };
    if odd as f64 / visible.len() as f64 > odd_limit { return false; }
    // Latin-script words without vowels ("WBT", "7Wk") are glyph codes once a line has
    // several of them; short lines of acronyms ("SN BSCS") stay readable.
    let words: Vec<&str> = line
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.chars().filter(|ch| ch.is_ascii_alphabetic()).count() >= 2)
        .collect();
    if words.len() >= 4 {
        let voiced = words.iter().filter(|word| word.chars().any(|ch| "aeiouyAEIOUY".contains(ch))).count();
        let mixed = words.iter().filter(|word| word.chars().any(|ch| ch.is_ascii_digit())).count();
        if (voiced as f64) < words.len() as f64 * 0.4 || mixed * 2 > words.len() { return false; }
    }
    true
}

/// The page text with unreadable lines removed, so they never reach the index.
fn readable_lines(text: &str) -> String {
    text.lines().filter(|line| is_readable_line(line)).collect::<Vec<_>>().join("\n")
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

    /// A page whose compressed content inflates to `inflated_bytes`, optionally after one
    /// readable text page. Under a smaller page limit its text cannot be extracted.
    pub fn pdf_with_oversized_page(inflated_bytes: usize, with_readable_page: bool) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let mut kids = Vec::new();
        if with_readable_page {
            let readable = Content { operations: vec![Operation::new("BT", vec![]), Operation::new("Tf", vec!["F1".into(), 12.into()]), Operation::new("Td", vec![72.into(), 760.into()]), Operation::new("Tj", vec![Object::string_literal("Readable page.")]), Operation::new("ET", vec![])] };
            let readable_id = doc.add_object(Stream::new(dictionary! {}, readable.encode().unwrap()));
            kids.push(doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => readable_id }).into());
        }
        let mut oversized = b"% ".to_vec();
        oversized.extend(std::iter::repeat_n(b'A', inflated_bytes));
        oversized.push(b'\n');
        let mut stream = Stream::new(dictionary! {}, oversized);
        stream.compress().unwrap();
        let oversized_id = doc.add_object(stream);
        kids.push(doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => oversized_id }).into());
        finish(doc, pages_id, kids, resources_id)
    }

    /// A text PDF saved with compressed object streams, carrying an object that inflates
    /// to `inflated_bytes` — a decompression bomb when that exceeds the load limit.
    pub fn pdf_with_large_object_stream(inflated_bytes: usize) -> Vec<u8> {
        let mut doc = Document::load_mem(&text_pdf(&[&["Bomb carrier."]])).unwrap();
        doc.add_object(Object::string_literal(vec![b'A'; inflated_bytes]));
        let mut bytes = Vec::new();
        doc.save_modern(&mut bytes).unwrap();
        bytes
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
        ExtractedText { text: text.to_owned(), pages: vec![(None, 0..text.len())], skipped_pages: Vec::new() }
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
    fn glyph_codes_are_unreadable_but_ordinary_lines_are_kept() {
        for garbled in ["&T˛˛˛˛˛m)7Wk˛7B_)Z˛#W)7˛WBT_W]B˛˛˛:•", "&Tm)7Wk 7B_)Z #W)7 WBT_W]B 7Wk", "\u{fffd}\u{fffd}\u{fffd}\u{fffd}", "\u{e001}\u{e002}\u{e003} ab"] {
            assert!(!is_readable_line(garbled), "{garbled}");
        }
        for readable in [
            "U.P. FORM 5A UNIVERSITY OF THE PHILIPPINES DILIMAN QUEZON CITY",
            "CLASSES TO BE ADDED AS ADVISED Remarks Checker OK OK OK OK",
            "Pirma bago ang interview, tapos i-submit na natin sa Oktubre 24.",
            "SN: 2021-12345 BSCS",
            "PHP 6,000   PHP 3,200   PHP 12,500",
            "Name: ____________________  Date: __/__/____",
            "• Bring your CBC and urinalysis results.",
            "",
        ] {
            assert!(is_readable_line(readable), "{readable}");
        }
    }

    #[test]
    fn unreadable_pdf_lines_are_not_indexed() {
        assert_eq!(readable_lines("Medical exam results\n&T˛˛˛˛˛m)7Wk˛7B_)Z˛#W)7˛WBT_W]B˛˛˛:•\nfit to work."), "Medical exam results\nfit to work.");
        let bytes = testpdf::text_pdf(&[&["Medical exam results: fit to work."], &["&Tm)7Wk 7B_)Z #W)7 WBT_W]B 7Wk"]]);
        let Extraction::Text(extracted) = extract(MediaKind::Pdf, &bytes).unwrap() else { panic!("expected text") };
        assert!(extracted.text.contains("fit to work"));
        assert!(!extracted.text.contains("WBT"), "{}", extracted.text);
        assert_eq!(extracted.skipped_pages, vec![2], "a page of only glyph codes is reported as not read");

        let unreadable = testpdf::text_pdf(&[&["&Tm)7Wk 7B_)Z #W)7 WBT_W]B 7Wk"]]);
        assert!(matches!(extract(MediaKind::Pdf, &unreadable).unwrap(), Extraction::Unsupported(reason) if reason.contains("unreadable")));
    }

    #[test]
    fn a_decompression_bomb_is_never_inflated_while_loading() {
        const LIMIT: usize = 64 * 1024;
        let bomb = testpdf::pdf_with_large_object_stream(4 * LIMIT);
        assert!(bomb.len() < LIMIT, "the object stream is compressed");
        let loaded = load_pdf(&bomb, LIMIT).unwrap();
        let largest = loaded.objects.values().filter_map(|object| object.as_str().ok()).map(<[u8]>::len).max().unwrap_or(0);
        assert!(largest <= LIMIT, "an object stream past the limit is dropped, not allocated");
        let unbounded = lopdf::Document::load_mem(&bomb).unwrap();
        assert!(unbounded.objects.values().filter_map(|object| object.as_str().ok()).any(|text| text.len() == 4 * LIMIT), "without the limit the same file inflates");
        assert_eq!(extract_pdf(&bomb, LIMIT).err().unwrap().code, ErrorCode::DocumentNotText, "a PDF whose pages were dropped is not called a scan");
    }

    #[test]
    fn a_page_that_cannot_be_read_is_skipped_not_called_a_scan() {
        let pdf = testpdf::pdf_with_oversized_page(256 * 1024, true);
        let Extraction::Text(extracted) = extract_pdf(&pdf, 64 * 1024).unwrap() else { panic!("the readable page is indexed") };
        assert!(extracted.text.contains("Readable page."));
        assert_eq!(extracted.skipped_pages, vec![2]);
    }

    #[test]
    fn a_pdf_whose_only_pages_fail_is_an_error_not_unsupported() {
        let pdf = testpdf::pdf_with_oversized_page(256 * 1024, false);
        let failure = extract_pdf(&pdf, 64 * 1024).err().unwrap();
        assert_eq!(failure.code, ErrorCode::DocumentNotText);
        assert!(failure.message.contains("could not be extracted"));
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
