use std::collections::HashSet;
use rusqlite::Connection;
use serde::Serialize;
use crate::error::NativeResult;
use crate::index::{self, DuplicateGroup};
use crate::workspace::ScopedRoot;

const MAX_SLUG_CHARS: usize = 60;

/// A proposed rename. Applying it goes through an approved plan like any other change.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FilenameSuggestion {
    pub document_id: String,
    pub relative_path: String,
    pub suggested_relative_path: String,
    pub content_hash: String,
    pub reason: String,
}

/// Organization Suggestions: exact duplicates are evidence only (never moved or deleted
/// automatically); filename suggestions become rename plans when the user picks them.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeSuggestions {
    pub duplicates: Vec<DuplicateGroup>,
    pub filenames: Vec<FilenameSuggestion>,
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

pub fn suggestions(conn: &Connection, root: &ScopedRoot) -> NativeResult<OrganizeSuggestions> {
    let documents = index::list_documents(conn, &root.id)?;
    let mut taken: HashSet<String> = documents.iter().map(|document| document.relative_path.to_lowercase()).collect();
    let mut filenames = Vec::new();
    for document in &documents {
        if document.status != "indexed" || document.media_type == "application/pdf" { continue; }
        let Some((stem, extension)) = document.name.rsplit_once('.') else { continue };
        let slug = slugify(&document.title);
        if slug.is_empty() || slug.chars().count() > MAX_SLUG_CHARS || slug == stem { continue; }
        let folder = document.relative_path.rsplit_once('/').map_or(String::new(), |(folder, _)| format!("{folder}/"));
        let suggested = format!("{folder}{slug}.{extension}");
        if !taken.insert(suggested.to_lowercase()) || root.path.join(&suggested).exists() { continue; }
        filenames.push(FilenameSuggestion {
            document_id: document.id.clone(),
            relative_path: document.relative_path.clone(),
            suggested_relative_path: suggested,
            content_hash: document.content_hash.clone(),
            reason: format!("Named after the document's title \u{201c}{}\u{201d}.", document.title),
        });
    }
    Ok(OrganizeSuggestions { duplicates: index::duplicate_groups(conn, root)?, filenames })
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
    fn suggestions_propose_without_changing_files() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before: Vec<_> = walkdir::WalkDir::new(folder.path()).into_iter().filter_map(Result::ok).map(|entry| entry.path().to_path_buf()).collect();
        let result = suggestions(&conn, &root).unwrap();
        assert_eq!(result.duplicates.len(), 1);
        let plan = result.filenames.iter().find(|suggestion| suggestion.relative_path == "projects/project-plan.md").unwrap();
        assert_eq!(plan.suggested_relative_path, "projects/community-learning-project.md");
        let copy = result.filenames.iter().find(|suggestion| suggestion.relative_path == "archive/project-plan-copy.md").unwrap();
        assert_eq!(copy.suggested_relative_path, "archive/community-learning-project.md");
        assert!(result.filenames.iter().all(|suggestion| !suggestion.suggested_relative_path.ends_with(".pdf")));
        let after: Vec<_> = walkdir::WalkDir::new(folder.path()).into_iter().filter_map(Result::ok).map(|entry| entry.path().to_path_buf()).collect();
        assert_eq!(before, after);
    }
}
