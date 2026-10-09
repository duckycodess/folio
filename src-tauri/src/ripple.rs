use std::collections::BTreeMap;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use crate::error::NativeResult;
use crate::extract::Utf16Cursor;
use crate::index::{self, IndexedDocument, SourcePassage};

const MAX_PASSAGES_PER_DOCUMENT: usize = 5;

/// A related document to review. Ripple never adds operations; candidates stay unchanged.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImpactCandidate {
    pub document_id: String,
    pub relative_path: String,
    pub reason: String,
    pub evidence: Vec<SourcePassage>,
    /// `evidence`: linked to the target and mentions the replaced value.
    /// `similarityOnly`: related some other way; no claim that it must change.
    pub strength: String,
}

#[derive(Default)]
struct Neighbor {
    links_to_target: bool,
    linked_from_target: bool,
    other_relationship: Option<String>,
}

/// Folio Ripple for replacing `old_value` in `target`:
/// documents that link to or from the target and mention the old value are evidence;
/// byte-identical copies and non-link relationships are similarity-only; documents that
/// merely share the value without any relationship are not reported.
pub fn impacts(conn: &Connection, workspace_id: &str, target: &IndexedDocument, old_value: &str) -> NativeResult<Vec<ImpactCandidate>> {
    let mut neighbors: BTreeMap<String, Neighbor> = BTreeMap::new();
    {
        let mut statement = conn.prepare("SELECT source_document_id, target_document_id, relationship_type FROM relationships WHERE source_document_id = ?1 OR target_document_id = ?1")?;
        let rows = statement.query_map([&target.id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?;
        for row in rows {
            let (source, destination, kind) = row?;
            let (other, outgoing) = if source == target.id { (destination, true) } else { (source, false) };
            if other == target.id { continue; }
            let entry = neighbors.entry(other).or_default();
            match (kind.as_str(), outgoing) {
                ("explicitReference", false) => entry.links_to_target = true,
                ("explicitReference", true) => entry.linked_from_target = true,
                _ => entry.other_relationship = Some(kind),
            }
        }
    }

    let mut candidates = Vec::new();
    for (document_id, neighbor) in &neighbors {
        let evidence = occurrences(conn, document_id, old_value)?;
        if evidence.is_empty() { continue; }
        let document = index::get_document(conn, workspace_id, document_id)?;
        let (strength, link) = match (neighbor.links_to_target, neighbor.linked_from_target, &neighbor.other_relationship) {
            (true, true, _) => ("evidence", "Links to and is linked from the target".to_owned()),
            (true, false, _) => ("evidence", "Links to the target".to_owned()),
            (false, true, _) => ("evidence", "Is linked from the target".to_owned()),
            (false, false, Some(kind)) => ("similarityOnly", format!("Related to the target by {kind}")),
            (false, false, None) => continue,
        };
        candidates.push(ImpactCandidate {
            document_id: document_id.clone(),
            relative_path: document.relative_path,
            reason: format!("{link} and mentions \u{201c}{old_value}\u{201d}."),
            evidence,
            strength: strength.into(),
        });
    }

    let mut statement = conn.prepare("SELECT id FROM documents WHERE workspace_id = ?1 AND content_hash = ?2 AND id != ?3 AND content_hash != '' ORDER BY relative_path")?;
    let copies: Vec<String> = statement.query_map(params![workspace_id, target.content_hash, target.id], |row| row.get(0))?.collect::<Result<_, _>>()?;
    for document_id in copies {
        if candidates.iter().any(|candidate| candidate.document_id == document_id) { continue; }
        let document = index::get_document(conn, workspace_id, &document_id)?;
        candidates.push(ImpactCandidate {
            evidence: occurrences(conn, &document_id, old_value)?,
            document_id,
            relative_path: document.relative_path,
            reason: "Byte-identical copy of the target before this edit; this plan does not change it.".into(),
            strength: "similarityOnly".into(),
        });
    }

    candidates.sort_by(|a, b| (a.strength != "evidence", &a.relative_path).cmp(&(b.strength != "evidence", &b.relative_path)));
    Ok(candidates)
}

/// The lines of a document's indexed text that contain `value`, located with UTF-16 offsets.
fn occurrences(conn: &Connection, document_id: &str, value: &str) -> NativeResult<Vec<SourcePassage>> {
    if value.is_empty() { return Ok(Vec::new()); }
    let mut statement = conn.prepare("SELECT chunk_text, start_offset, page FROM chunks WHERE document_id = ?1 AND instr(chunk_text, ?2) > 0 ORDER BY ordinal")?;
    let rows = statement.query_map(params![document_id, value], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<u32>>(2)?)))?;
    let mut passages: Vec<SourcePassage> = Vec::new();
    for row in rows {
        let (text, chunk_start, page) = row?;
        let mut cursor = Utf16Cursor::new(&text);
        for (index, _) in text.match_indices(value) {
            let line_start = text[..index].rfind('\n').map_or(0, |newline| newline + 1);
            let line_end = text[index..].find('\n').map_or(text.len(), |newline| index + newline);
            let line = &text[line_start..line_end];
            let leading = line.len() - line.trim_start().len();
            let (from, to) = (line_start + leading, line_start + line.trim_end().len());
            let passage = SourcePassage {
                document_id: document_id.to_owned(),
                start: chunk_start as usize + cursor.at(from),
                end: chunk_start as usize + cursor.at(to),
                text: text[from..to].to_owned(),
                page,
            };
            if !passages.iter().any(|existing| existing.start == passage.start) { passages.push(passage); }
            if passages.len() >= MAX_PASSAGES_PER_DOCUMENT { return Ok(passages); }
        }
    }
    Ok(passages)
}
