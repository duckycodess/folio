use std::collections::BTreeMap;
use rusqlite::{params, Connection};
use crate::contracts::{FileOperation, ImpactCandidate, ImpactStrength, RelationshipKind, RelationshipProvenance, SourcePassage};
use crate::db::NativeResult;
use crate::extract::line_bounds;
use crate::index::{self, IndexedDocument};
use crate::workspace::{self, ScopedRoot};

/// Ripple is a review aid with a bounded answer: the strongest candidates first.
const MAX_CANDIDATES: usize = 25;
const MAX_PASSAGES_PER_DOCUMENT: usize = 5;
const MAX_PHRASE_CHARS: usize = 120;

/// English full name, English abbreviation, Filipino name.
const MONTHS: [[&str; 3]; 12] = [
    ["january", "jan", "enero"],
    ["february", "feb", "pebrero"],
    ["march", "mar", "marso"],
    ["april", "apr", "abril"],
    ["may", "may", "mayo"],
    ["june", "jun", "hunyo"],
    ["july", "jul", "hulyo"],
    ["august", "aug", "agosto"],
    ["september", "sep", "setyembre"],
    ["october", "oct", "oktubre"],
    ["november", "nov", "nobyembre"],
    ["december", "dec", "disyembre"],
];

fn month_index(word: &str) -> Option<usize> {
    let word = word.trim_end_matches('.').to_lowercase();
    MONTHS.iter().position(|names| names.contains(&word.as_str()))
}

/// Lowercases character by character where that keeps the byte length, so offsets in
/// the folded text are offsets in the original.
fn fold(text: &str) -> String {
    text.chars()
        .map(|ch| {
            let mut lower = ch.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(single), None) if single.len_utf8() == ch.len_utf8() => single,
                _ => ch,
            }
        })
        .collect()
}

/// The phrase to look for, plus the same date written with the other month names
/// Folio's users write ("October 20", "Oct 20", "Oktubre 20").
pub fn variants(phrase: &str) -> Vec<String> {
    let base = fold(phrase.trim());
    let mut found = vec![base.clone()];
    let words: Vec<&str> = base.split_whitespace().collect();
    if let [month, day] = words.as_slice() {
        if let (Some(index), true) = (month_index(month), day.chars().all(|ch| ch.is_ascii_digit()) && day.len() <= 2) {
            for name in MONTHS[index] {
                let variant = format!("{name} {day}");
                if !found.contains(&variant) { found.push(variant); }
            }
        }
    }
    found
}

/// Byte offsets of `needle` in `haystack` where it is a whole phrase: "October 20" does
/// not match inside "October 2026".
fn phrase_positions(haystack: &str, needle: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    if needle.is_empty() { return positions; }
    for (start, _) in haystack.match_indices(needle) {
        let end = start + needle.len();
        let before = haystack[..start].chars().next_back().is_none_or(|ch| !ch.is_alphanumeric());
        let after = haystack[end..].chars().next().is_none_or(|ch| !ch.is_alphanumeric());
        if before && after { positions.push(start); }
    }
    positions
}

/// The phrase an edit replaces, from the difference between the current and the new
/// text, widened to whole words and to a date's month name. `None` for a pure insertion.
pub fn replaced_phrase(before: &str, after: &str) -> Option<String> {
    let prefix = before.char_indices().zip(after.chars()).take_while(|((_, a), b)| a == b).last().map_or(0, |((index, ch), _)| index + ch.len_utf8());
    let suffix = before[prefix..].chars().rev().zip(after[prefix.min(after.len())..].chars().rev()).take_while(|(a, b)| a == b).map(|(ch, _)| ch.len_utf8()).sum::<usize>();
    let (mut start, mut end) = (prefix, before.len() - suffix);
    if start >= end { return None; }
    while let Some(ch) = before[..start].chars().next_back().filter(|ch| ch.is_alphanumeric()) { start -= ch.len_utf8(); }
    while let Some(ch) = before[end..].chars().next().filter(|ch| ch.is_alphanumeric()) { end += ch.len_utf8(); }
    let phrase = &before[start..end];
    if phrase.chars().all(|ch| ch.is_ascii_digit()) {
        // "20" in "October 20" is the date "October 20".
        let lead = before[..start].trim_end_matches(' ');
        let word_start = lead.rfind(|ch: char| !ch.is_alphanumeric() && ch != '.').map_or(0, |index| index + 1);
        if lead.len() < start && month_index(&lead[word_start..]).is_some() { start = word_start; }
    } else if month_index(phrase).is_some() {
        let rest = &before[end..];
        let digits: usize = rest.strip_prefix(' ').map_or(0, |tail| tail.chars().take_while(|ch| ch.is_ascii_digit()).count());
        if (1..=2).contains(&digits) { end += 1 + digits; }
    }
    let phrase = before[start..end].trim();
    (!phrase.is_empty() && phrase.chars().count() <= MAX_PHRASE_CHARS).then(|| phrase.to_owned())
}

#[derive(Default)]
struct Neighbor {
    links_to_target: bool,
    linked_from_target: bool,
    shared_fact: Option<RelationshipProvenance>,
    similarity: bool,
}

fn provenance(value: &str) -> RelationshipProvenance {
    match value {
        "documentLink" => RelationshipProvenance::DocumentLink,
        "model" => RelationshipProvenance::Model,
        _ => RelationshipProvenance::Embedding,
    }
}

/// Folio Ripple for replacing `replaced` in `target`. Candidates are review evidence and
/// never become operations:
/// - linked to or from the target and mentioning the replaced value: `evidence`;
/// - a shared-fact candidate mentioning the value: `evidence`;
/// - a similarity relationship or a byte-identical copy: `similarityOnly`;
/// - a document that merely shares the value, with no relationship: not reported.
pub fn impacts(conn: &Connection, workspace_id: &str, target: &IndexedDocument, replaced: &str) -> NativeResult<Vec<ImpactCandidate>> {
    let phrases = variants(replaced);
    let mut neighbors: BTreeMap<String, Neighbor> = BTreeMap::new();
    {
        let mut statement = conn.prepare("SELECT source_document_id, target_document_id, relationship_type, provenance FROM relationships WHERE source_document_id = ?1 OR target_document_id = ?1")?;
        let rows = statement.query_map([&target.id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)))?;
        for row in rows {
            let (source, destination, kind, origin) = row?;
            let (other, outgoing) = if source == target.id { (destination, true) } else { (source, false) };
            if other == target.id { continue; }
            let entry = neighbors.entry(other).or_default();
            match kind.as_str() {
                "explicitReference" if outgoing => entry.linked_from_target = true,
                "explicitReference" => entry.links_to_target = true,
                "sharedFactCandidate" => entry.shared_fact = Some(provenance(&origin)),
                _ => entry.similarity = true,
            }
        }
    }

    let mut candidates = Vec::new();
    let mention = format!("\u{201c}{}\u{201d}", replaced.trim());
    for (document_id, neighbor) in &neighbors {
        let evidence = occurrences(conn, document_id, &phrases)?;
        if evidence.is_empty() { continue; }
        let (strength, kind, origin, reason) = match neighbor {
            Neighbor { links_to_target: true, linked_from_target: true, .. } => (ImpactStrength::Evidence, RelationshipKind::ExplicitReference, RelationshipProvenance::DocumentLink, "Links to and is linked from the target"),
            Neighbor { links_to_target: true, .. } => (ImpactStrength::Evidence, RelationshipKind::ExplicitReference, RelationshipProvenance::DocumentLink, "Links to the target"),
            Neighbor { linked_from_target: true, .. } => (ImpactStrength::Evidence, RelationshipKind::ExplicitReference, RelationshipProvenance::DocumentLink, "Is linked from the target"),
            Neighbor { shared_fact: Some(origin), .. } => (ImpactStrength::Evidence, RelationshipKind::SharedFactCandidate, *origin, "May state the same fact as the target"),
            _ => (ImpactStrength::SimilarityOnly, RelationshipKind::Similarity, RelationshipProvenance::Embedding, "Covers similar subject matter"),
        };
        let document = index::get_document(conn, workspace_id, document_id)?;
        candidates.push(ImpactCandidate {
            document_id: document_id.clone(),
            relative_path: document.relative_path,
            reason: format!("{reason} and mentions {mention}."),
            evidence,
            strength,
            relationship_type: Some(kind),
            provenance: Some(origin),
        });
    }

    let mut statement = conn.prepare("SELECT id FROM documents WHERE workspace_id = ?1 AND content_hash = ?2 AND id != ?3 AND content_hash != '' ORDER BY relative_path")?;
    let copies: Vec<String> = statement.query_map(params![workspace_id, target.content_hash, target.id], |row| row.get(0))?.collect::<Result<_, _>>()?;
    for document_id in copies {
        if candidates.iter().any(|candidate| candidate.document_id == document_id) { continue; }
        let document = index::get_document(conn, workspace_id, &document_id)?;
        candidates.push(ImpactCandidate {
            evidence: occurrences(conn, &document_id, &phrases)?,
            document_id,
            relative_path: document.relative_path,
            reason: "Byte-identical copy of the target before this edit; this plan does not change it.".into(),
            strength: ImpactStrength::SimilarityOnly,
            relationship_type: None,
            provenance: None,
        });
    }

    candidates.sort_by(|a, b| (a.strength != ImpactStrength::Evidence, &a.relative_path).cmp(&(b.strength != ImpactStrength::Evidence, &b.relative_path)));
    candidates.truncate(MAX_CANDIDATES);
    Ok(candidates)
}

/// Ripple for every edit in a plan: the replaced phrase comes from each edit's diff.
/// Documents the plan itself changes are not listed as candidates.
pub fn plan_impacts(conn: &Connection, root: &ScopedRoot, operations: &[FileOperation]) -> NativeResult<Vec<ImpactCandidate>> {
    let targeted: Vec<&str> = operations.iter().filter_map(|operation| match operation {
        FileOperation::Edit { document_id, .. } | FileOperation::Rename { document_id, .. } | FileOperation::Move { document_id, .. } => Some(document_id.as_str()),
        FileOperation::Create { .. } => None,
    }).collect();
    let mut found: Vec<ImpactCandidate> = Vec::new();
    for operation in operations {
        let FileOperation::Edit { document_id, relative_path, after, .. } = operation else { continue };
        let Ok(document) = index::get_document(conn, &root.id, document_id) else { continue };
        let Ok(current) = workspace::read_text(&root.path, relative_path) else { continue };
        let Some(phrase) = replaced_phrase(&current.content, after) else { continue };
        for candidate in impacts(conn, &root.id, &document, &phrase)? {
            if !targeted.contains(&candidate.document_id.as_str()) && !found.iter().any(|existing| existing.document_id == candidate.document_id) {
                found.push(candidate);
            }
        }
    }
    found.truncate(MAX_CANDIDATES);
    Ok(found)
}

/// The lines of a document's indexed text that mention any of `phrases` as whole phrases.
fn occurrences(conn: &Connection, document_id: &str, phrases: &[String]) -> NativeResult<Vec<SourcePassage>> {
    let mut statement = conn.prepare("SELECT c.chunk_text, c.start_offset, c.page, d.content_hash FROM chunks c JOIN documents d ON d.id = c.document_id WHERE c.document_id = ?1 ORDER BY c.ordinal")?;
    let rows = statement.query_map([document_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<u32>>(2)?, row.get::<_, String>(3)?)))?;
    let mut passages: Vec<SourcePassage> = Vec::new();
    for row in rows {
        let (text, chunk_start, page, hash) = row?;
        let folded = fold(&text);
        let mut hits: Vec<(usize, usize)> = phrases.iter().flat_map(|phrase| phrase_positions(&folded, phrase).into_iter().map(move |start| (start, start + phrase.len()))).collect();
        hits.sort_unstable();
        for (start, end) in hits {
            let (from, to) = line_bounds(&text, start, end);
            let start = chunk_start as usize + from;
            if passages.iter().any(|existing| existing.start == start) { continue; }
            passages.push(index::passage(document_id, &hash, start, chunk_start as usize + to, &text[from..to], page));
            if passages.len() >= MAX_PASSAGES_PER_DOCUMENT { return Ok(passages); }
        }
    }
    Ok(passages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replaced_phrase_is_widened_to_words_and_dates() {
        let before = "The project submission deadline is October 20. The team will present.";
        let after = before.replace("October 20", "October 23");
        assert_eq!(replaced_phrase(before, &after).as_deref(), Some("October 20"));
        assert_eq!(replaced_phrase("Due Oktubre 20.", "Due Nobyembre 20.").as_deref(), Some("Oktubre 20"));
        assert_eq!(replaced_phrase("Meet Maya today.", "Meet Nico today.").as_deref(), Some("Maya"));
        assert_eq!(replaced_phrase("abc", "abcdef"), None, "a pure insertion replaces nothing");
    }

    #[test]
    fn dates_match_in_english_and_filipino_but_not_as_a_prefix() {
        let phrases = variants("October 20");
        assert!(phrases.contains(&"oktubre 20".to_owned()) && phrases.contains(&"oct 20".to_owned()));
        let text = fold("Ang huling araw ay Oktubre 20. Ang kumperensya ay October 2026.");
        let hits: Vec<usize> = phrases.iter().flat_map(|phrase| phrase_positions(&text, phrase)).collect();
        assert_eq!(hits.len(), 1, "October 2026 is a different value");
    }

    #[test]
    fn folding_keeps_byte_offsets() {
        let text = "İstanbul — ÑANDÚ October";
        assert_eq!(fold(text).len(), text.len());
    }
}
