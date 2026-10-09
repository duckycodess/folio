use std::collections::HashSet;
use rusqlite::Connection;
use serde::Serialize;
use crate::contracts::{DestinationState, FileOperation};
use crate::db::NativeResult;
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
}
