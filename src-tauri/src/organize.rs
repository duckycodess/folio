use std::collections::HashSet;
use folio_core::contracts::{DocumentRecord, SourcePassage};
use folio_core::file_suggestions::{DestinationCandidate, GeneratedFilename};
use rusqlite::Connection;
use serde::Serialize;
use crate::contracts::{DestinationState, FileOperation};
use crate::db::NativeResult;
use crate::identity::{assert_portable_destination, is_editable_media_type};
use crate::index::{self, DuplicateGroup};
use crate::workspace::ScopedRoot;

const MAX_SLUG_CHARS: usize = 60;

/// An Organization Suggestion for a filename. It changes nothing by itself: its
/// `operation` is previewed and approved like any other plan.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationSuggestion {
    pub document_id: String,
    pub relative_path: String,
    pub suggested_relative_path: String,
    pub reason: String,
    pub operation: FileOperation,
    /// Present only when the local model wrote the name; title-based names have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated: Option<GeneratedBy>,
}

/// Which local model wrote a suggestion, and the passages it was based on.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedBy {
    pub citations: Vec<SourcePassage>,
    pub model_id: String,
    pub revision: String,
}

/// Exact duplicates are evidence only — never moved or deleted automatically — and
/// filename suggestions become rename plans only when the user picks one.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationSuggestions {
    pub duplicate_groups: Vec<DuplicateGroup>,
    pub filenames: Vec<OrganizationSuggestion>,
}

/// Lowercase words joined by hyphens; letters with accents and non-Latin letters are kept.
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    for ch in title.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            slug.push(ch);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_owned()
}

/// Filename suggestions, from the index. Cheap enough to run while the index is held.
pub fn filename_suggestions(conn: &Connection, root: &ScopedRoot) -> NativeResult<Vec<OrganizationSuggestion>> {
    let documents = index::list_documents(conn, &root.id)?;
    let mut taken: HashSet<String> = documents.iter().map(|document| document.relative_path.to_lowercase()).collect();
    let mut filenames = Vec::new();
    for document in &documents {
        if document.status != "indexed" || document.media_type == "application/pdf" { continue; }
        let Some((stem, extension)) = document.name.rsplit_once('.') else { continue };
        let slug = slugify(&document.title);
        if slug.is_empty() || slug.chars().count() > MAX_SLUG_CHARS || slug == stem.to_lowercase() { continue; }
        let folder = document.relative_path.rsplit_once('/').map_or(String::new(), |(folder, _)| format!("{folder}/"));
        let suggested = format!("{folder}{slug}.{extension}");
        if !taken.insert(suggested.to_lowercase()) || root.path.join(&suggested).exists() { continue; }
        filenames.push(OrganizationSuggestion {
            document_id: document.id.clone(),
            relative_path: document.relative_path.clone(),
            suggested_relative_path: suggested.clone(),
            reason: format!("Named after the document's title \u{201c}{}\u{201d}.", document.title),
            generated: None,
            operation: FileOperation::Rename {
                document_id: document.id.clone(),
                relative_path: document.relative_path.clone(),
                expected_content_hash: document.content_hash.clone(),
                destination_relative_path: suggested,
                expected_destination: DestinationState::Absent,
            },
        });
    }
    Ok(filenames)
}

/// A free relative path for a renamed or moved file, or `None` when the name is
/// unusable, unchanged, already taken, or not portable.
fn free_destination(root: &ScopedRoot, current: &str, suggested: String, taken: &mut HashSet<String>) -> Option<String> {
    if suggested.to_lowercase() == current.to_lowercase() || assert_portable_destination(&suggested).is_err() {
        return None;
    }
    if !taken.insert(suggested.to_lowercase()) || root.path.join(&suggested).exists() {
        return None;
    }
    Some(suggested)
}

fn split_name(relative_path: &str) -> Option<(&str, &str, &str)> {
    let (folder, name) = relative_path.rsplit_once('/').map_or(("", relative_path), |(folder, name)| (folder, name));
    let (stem, extension) = name.rsplit_once('.')?;
    Some((folder, stem, extension))
}

/// Model-written names as rename suggestions, keeping each file's folder and
/// extension. `documents` are the files the names were written from, with the
/// revision read; the rename is refused later if a file changed since.
pub fn model_filenames(root: &ScopedRoot, documents: &[DocumentRecord], generated: &[GeneratedFilename]) -> Vec<OrganizationSuggestion> {
    let mut taken: HashSet<String> = documents.iter().map(|document| document.relative_path.to_lowercase()).collect();
    let mut suggestions = Vec::new();
    for name in generated {
        let Some(document) = documents.iter().find(|document| document.id == name.document_id) else { continue };
        let Some(hash) = document.content_hash.clone() else { continue };
        let Some((folder, _, extension)) = split_name(&document.relative_path) else { continue };
        let slug = slugify(&name.text);
        if slug.is_empty() || slug.chars().count() > MAX_SLUG_CHARS || !is_editable_media_type(&document.media_type) {
            continue;
        }
        let suggested = if folder.is_empty() { format!("{slug}.{extension}") } else { format!("{folder}/{slug}.{extension}") };
        let Some(suggested) = free_destination(root, &document.relative_path, suggested, &mut taken) else { continue };
        suggestions.push(OrganizationSuggestion {
            document_id: document.id.clone(),
            relative_path: document.relative_path.clone(),
            suggested_relative_path: suggested.clone(),
            reason: format!("Named by the local AI from the file's contents: \u{201c}{}\u{201d}.", name.text),
            operation: FileOperation::Rename {
                document_id: document.id.clone(),
                relative_path: document.relative_path.clone(),
                expected_content_hash: hash,
                destination_relative_path: suggested,
                expected_destination: DestinationState::Absent,
            },
            generated: Some(GeneratedBy { citations: name.citations.clone(), model_id: name.model_id.clone(), revision: name.revision.clone() }),
        });
    }
    suggestions
}

/// A move into an existing folder whose files are closer in meaning. The folder
/// must already exist: Folio does not create folders.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DestinationSuggestion {
    pub document_id: String,
    pub relative_path: String,
    pub suggested_relative_path: String,
    pub folder: String,
    pub reason: String,
    pub similarity: f32,
    pub current_similarity: f32,
    pub passage: SourcePassage,
    pub evidence: SourcePassage,
    pub provenance: &'static str,
    pub space_fingerprint: String,
    pub operation: FileOperation,
}

pub fn destinations(root: &ScopedRoot, documents: &[DocumentRecord], candidates: Vec<DestinationCandidate>) -> Vec<DestinationSuggestion> {
    let mut taken: HashSet<String> = documents.iter().map(|document| document.relative_path.to_lowercase()).collect();
    let mut suggestions = Vec::new();
    for candidate in candidates {
        let Some(document) = documents.iter().find(|document| document.id == candidate.document_id) else { continue };
        let Some(hash) = document.content_hash.clone() else { continue };
        if !is_editable_media_type(&document.media_type) || (!candidate.folder.is_empty() && !root.path.join(&candidate.folder).is_dir()) {
            continue;
        }
        let name = document.relative_path.rsplit('/').next().unwrap_or(&document.relative_path);
        let suggested = if candidate.folder.is_empty() { name.to_owned() } else { format!("{}/{name}", candidate.folder) };
        let Some(suggested) = free_destination(root, &document.relative_path, suggested, &mut taken) else { continue };
        let place = if candidate.folder.is_empty() { "the top of the folder".to_owned() } else { format!("\u{201c}{}\u{201d}", candidate.folder) };
        suggestions.push(DestinationSuggestion {
            document_id: document.id.clone(),
            relative_path: document.relative_path.clone(),
            suggested_relative_path: suggested.clone(),
            folder: candidate.folder,
            reason: format!("Closer in meaning to the files in {place} than to the files beside it."),
            similarity: candidate.similarity,
            current_similarity: candidate.current_similarity,
            passage: candidate.passage,
            evidence: candidate.evidence,
            provenance: "embedding",
            space_fingerprint: candidate.space_fingerprint,
            operation: FileOperation::Move {
                document_id: document.id.clone(),
                relative_path: document.relative_path.clone(),
                expected_content_hash: hash,
                destination_relative_path: suggested,
                expected_destination: DestinationState::Absent,
            },
        });
    }
    suggestions
}

/// Analyzing a collection: filename suggestions for its members only, and the
/// duplicate groups that include one of them (with every copy, wherever it is).
pub fn limit_to(mut suggestions: OrganizationSuggestions, members: Option<&HashSet<String>>) -> OrganizationSuggestions {
    if let Some(members) = members {
        suggestions.filenames.retain(|suggestion| members.contains(&suggestion.document_id));
        suggestions.duplicate_groups.retain(|group| group.documents.iter().any(|document| members.contains(&document.id)));
    }
    suggestions
}

#[cfg(test)]
pub fn suggestions(conn: &Connection, root: &ScopedRoot) -> NativeResult<OrganizationSuggestions> {
    let filenames = filename_suggestions(conn, root)?;
    Ok(OrganizationSuggestions { duplicate_groups: index::verify_duplicates(&root.path, index::duplicate_candidates(conn, &root.id)?), filenames })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::tests::{fixture_workspace, scan};

    #[test]
    fn slugs_keep_letters_and_normalize_punctuation() {
        assert_eq!(slugify("Community Learning Project"), "community-learning-project");
        assert_eq!(slugify("  Tala sa Proyekto — Ikalawa!  "), "tala-sa-proyekto-ikalawa");
        assert_eq!(slugify("Pagsasanay sa Matemátika"), "pagsasanay-sa-matemátika");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn suggestions_propose_rename_operations_without_changing_files() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let listing = |path: &std::path::Path| -> Vec<_> { walkdir::WalkDir::new(path).into_iter().filter_map(Result::ok).map(|entry| entry.path().to_path_buf()).collect() };
        let before = listing(folder.path());
        let result = suggestions(&conn, &root).unwrap();
        assert_eq!(result.duplicate_groups.len(), 1);
        let plan = result.filenames.iter().find(|suggestion| suggestion.relative_path == "projects/project-plan.md").unwrap();
        assert_eq!(plan.suggested_relative_path, "projects/community-learning-project.md");
        assert!(matches!(&plan.operation, FileOperation::Rename { expected_content_hash, .. } if expected_content_hash.starts_with("sha256:")));
        assert!(result.filenames.iter().all(|suggestion| !suggestion.suggested_relative_path.ends_with(".pdf")));
        assert_eq!(listing(folder.path()), before);
    }

    #[test]
    fn analyzing_a_collection_limits_suggestions_to_its_members() {
        let (_folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let all = suggestions(&conn, &root).unwrap();
        assert!(all.filenames.len() > 1);
        let plan = crate::index::tests::id_of(&root, "projects/project-plan.md");
        let members = HashSet::from([plan.clone()]);
        let limited = limit_to(suggestions(&conn, &root).unwrap(), Some(&members));
        assert!(limited.filenames.iter().all(|suggestion| suggestion.document_id == plan));
        assert_eq!(limited.filenames.len(), 1);
        // The plan's identical copy outside the collection is still shown with it.
        assert_eq!(limited.duplicate_groups.len(), 1);
        assert_eq!(limited.duplicate_groups[0].documents.len(), 2);

        let elsewhere = HashSet::from([crate::index::tests::id_of(&root, "personal/grocery-list.md")]);
        assert!(limit_to(suggestions(&conn, &root).unwrap(), Some(&elsewhere)).duplicate_groups.is_empty());
        assert_eq!(limit_to(suggestions(&conn, &root).unwrap(), None).filenames.len(), all.filenames.len());
    }

    fn record(root: &ScopedRoot, path: &str) -> DocumentRecord {
        let text = crate::workspace::read_text(&root.path, path).unwrap();
        DocumentRecord {
            id: crate::index::tests::id_of(root, path),
            workspace_id: root.id.clone(),
            relative_path: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            title: path.into(),
            language: folio_core::contracts::Language::Unknown,
            media_type: crate::identity::media_type_for_path(path).unwrap().into(),
            size_bytes: text.size_bytes,
            modified_at_ms: None,
            content: Some(text.content),
            content_hash: Some(text.content_hash),
        }
    }

    fn generated(document: &DocumentRecord, text: &str) -> GeneratedFilename {
        GeneratedFilename { document_id: document.id.clone(), text: text.into(), citations: Vec::new(), model_id: "qwen".into(), revision: "r".into() }
    }

    #[test]
    fn model_names_become_renames_that_keep_the_folder_and_extension() {
        let (folder, _conn, root) = fixture_workspace();
        std::fs::write(folder.path().join("notes/untitled.md"), "Mga gastos sa biyahe.").unwrap();
        std::fs::write(folder.path().join("notes/draft.txt"), "Trip budget.").unwrap();
        let (untitled, draft, plan) = (record(&root, "notes/untitled.md"), record(&root, "notes/draft.txt"), record(&root, "projects/project-plan.md"));
        let pdf = record(&root, "research/consent-form-guide.pdf");
        let documents = vec![untitled.clone(), draft.clone(), plan.clone(), pdf.clone()];
        let names = vec![
            // notes/paalala.md already exists, so this is dropped rather than overwriting it.
            generated(&untitled, "Paalala"),
            generated(&draft, "Gastos sa Biyahe"),
            generated(&pdf, "Consent guide"),
            generated(&plan, "   "),
        ];
        let suggestions = model_filenames(&root, &documents, &names);
        assert_eq!(suggestions.len(), 1, "{suggestions:?}");
        let rename = &suggestions[0];
        assert_eq!(rename.suggested_relative_path, "notes/gastos-sa-biyahe.txt");
        assert!(rename.generated.is_some(), "a model-written name is labelled as generated");
        assert!(matches!(&rename.operation, FileOperation::Rename { expected_content_hash, .. } if Some(expected_content_hash) == draft.content_hash.as_ref()));
        // A .txt file keeps its extension; the same words in another folder are free.
        let draft_only = model_filenames(&root, &[draft.clone()], &[generated(&draft, "Project plan")]);
        assert_eq!(draft_only[0].suggested_relative_path, "notes/project-plan.txt");
        // Two names for the same new path: only the first is kept.
        let both = model_filenames(&root, &documents, &[generated(&untitled, "Trip"), generated(&draft, "Trip")]);
        assert_eq!(both.iter().map(|item| item.suggested_relative_path.as_str()).collect::<Vec<_>>(), ["notes/trip.md", "notes/trip.txt"]);
        assert!(model_filenames(&root, &documents, &[generated(&draft, "Draft")]).is_empty(), "an unchanged name is not a suggestion");
    }

    #[test]
    fn destinations_move_into_an_existing_folder_without_replacing_a_file() {
        let (_folder, _conn, root) = fixture_workspace();
        let notes = record(&root, "meetings/meeting-notes.md");
        let copy = record(&root, "archive/project-plan-copy.md");
        let pdf = record(&root, "research/consent-form-guide.pdf");
        let candidate = |document: &DocumentRecord, folder: &str| DestinationCandidate {
            document_id: document.id.clone(),
            relative_path: document.relative_path.clone(),
            folder: folder.into(),
            similarity: 0.95,
            current_similarity: 0.5,
            passage: SourcePassage { document_id: document.id.clone(), document_content_hash: document.content_hash.clone().unwrap(), offset_unit: Default::default(), start: 0, end: 1, text: "#".into(), page: None },
            evidence: SourcePassage { document_id: document.id.clone(), document_content_hash: document.content_hash.clone().unwrap(), offset_unit: Default::default(), start: 0, end: 1, text: "#".into(), page: None },
            space_fingerprint: "space".into(),
        };
        let found = destinations(
            &root,
            &[notes.clone(), copy.clone(), pdf.clone()],
            vec![candidate(&notes, "projects"), candidate(&copy, "missing-folder"), candidate(&pdf, "projects")],
        );
        assert_eq!(found.len(), 1, "a folder that doesn't exist, and a PDF, are never destinations");
        assert_eq!(found[0].suggested_relative_path, "projects/meeting-notes.md");
        assert_eq!(found[0].provenance, "embedding");
        assert!(matches!(&found[0].operation, FileOperation::Move { destination_relative_path, .. } if destination_relative_path == "projects/meeting-notes.md"));
        // A same-named file already in the folder is never replaced.
        std::fs::write(root.path.join("projects/meeting-notes.md"), "occupied").unwrap();
        assert!(destinations(&root, &[notes.clone()], vec![candidate(&notes, "projects")]).is_empty());
    }
}
