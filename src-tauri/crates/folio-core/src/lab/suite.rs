//! The fixed suite and corpus, embedded so a run never depends on the working
//! directory and every record can name exactly what it ran.
//!
//! Line endings are normalised to `\n` before hashing or use, so a Windows
//! checkout that converted them produces the same hashes as every other host.

use crate::error::{CoreError, CoreResult};
use crate::lab::record::SuiteRef;
use crate::models::sha256_bytes;
use serde::Deserialize;

pub const SUITE_ID: &str = "benchmark-cases";
const SUITE_JSON: &str = include_str!("../../../../../fixtures/benchmark-cases.json");

/// Text documents of `fixtures/documents`. The text-based PDF is left out: the
/// interim chunker used by the providers reads text only.
const CORPUS: &[(&str, &str)] = &[
    (
        "archive/project-plan-copy.md",
        include_str!("../../../../../fixtures/documents/archive/project-plan-copy.md"),
    ),
    (
        "courses/math-review.md",
        include_str!("../../../../../fixtures/documents/courses/math-review.md"),
    ),
    (
        "courses/pagsasanay-sa-math.md",
        include_str!("../../../../../fixtures/documents/courses/pagsasanay-sa-math.md"),
    ),
    (
        "meetings/meeting-notes.md",
        include_str!("../../../../../fixtures/documents/meetings/meeting-notes.md"),
    ),
    (
        "notes/paalala.md",
        include_str!("../../../../../fixtures/documents/notes/paalala.md"),
    ),
    (
        "notes/study-session.md",
        include_str!("../../../../../fixtures/documents/notes/study-session.md"),
    ),
    (
        "notes/tala-sa-proyekto.md",
        include_str!("../../../../../fixtures/documents/notes/tala-sa-proyekto.md"),
    ),
    (
        "personal/budget-notes.md",
        include_str!("../../../../../fixtures/documents/personal/budget-notes.md"),
    ),
    (
        "personal/grocery-list.md",
        include_str!("../../../../../fixtures/documents/personal/grocery-list.md"),
    ),
    (
        "personal/travel-notes.md",
        include_str!("../../../../../fixtures/documents/personal/travel-notes.md"),
    ),
    (
        "projects/project-plan.md",
        include_str!("../../../../../fixtures/documents/projects/project-plan.md"),
    ),
    (
        "projects/submission-checklist.md",
        include_str!("../../../../../fixtures/documents/projects/submission-checklist.md"),
    ),
    (
        "research/methodology-notes.md",
        include_str!("../../../../../fixtures/documents/research/methodology-notes.md"),
    ),
    (
        "research/review-reminders.md",
        include_str!("../../../../../fixtures/documents/research/review-reminders.md"),
    ),
    (
        "research/tala-sa-pamamaraan.md",
        include_str!("../../../../../fixtures/documents/research/tala-sa-pamamaraan.md"),
    ),
];

fn normalise_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "task", rename_all = "camelCase")]
pub enum SuiteCase {
    #[serde(rename_all = "camelCase")]
    Retrieval {
        id: String,
        input: String,
        relevant_documents: Vec<String>,
        #[serde(default)]
        negative_documents: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Summary {
        id: String,
        input: String,
        document: String,
        required_facts: Vec<String>,
        require_sources: bool,
    },
    #[serde(rename_all = "camelCase")]
    Interpretation {
        id: String,
        input: String,
        expected_operation: String,
        expected_target: String,
        expected_before: String,
        expected_after: String,
    },
    #[serde(rename_all = "camelCase")]
    Edit {
        id: String,
        input: String,
        expected_target: String,
        expected_after: String,
        review_candidates: Vec<String>,
        unrelated_same_date: Vec<String>,
        unchanged_related_files: bool,
    },
}

impl SuiteCase {
    pub fn id(&self) -> &str {
        match self {
            Self::Retrieval { id, .. }
            | Self::Summary { id, .. }
            | Self::Interpretation { id, .. }
            | Self::Edit { id, .. } => id,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Suite {
    pub id: &'static str,
    pub sha256: String,
    pub cases: Vec<SuiteCase>,
}

impl Suite {
    /// The suite is not frozen: it is the demonstration and development suite
    /// until a held-out suite is authored and frozen by its reviewer.
    pub fn embedded() -> CoreResult<Self> {
        Self::from_json(SUITE_JSON)
    }

    pub fn from_json(json: &str) -> CoreResult<Self> {
        let json = normalise_newlines(json);
        let cases: Vec<SuiteCase> = serde_json::from_str(&json)?;
        let mut seen = std::collections::HashSet::new();
        for case in &cases {
            if !seen.insert(case.id().to_string()) {
                return Err(CoreError::Message(format!(
                    "duplicate benchmark case id {}",
                    case.id()
                )));
            }
        }
        Ok(Self {
            id: SUITE_ID,
            sha256: sha256_bytes(json.as_bytes()),
            cases,
        })
    }

    pub fn reference(&self) -> SuiteRef {
        SuiteRef {
            id: self.id.to_string(),
            sha256: self.sha256.clone(),
            frozen: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CorpusDocument {
    /// `/`-separated path below the corpus root; also the lab's document id.
    pub relative_path: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct Corpus {
    pub documents: Vec<CorpusDocument>,
    pub sha256: String,
}

impl Corpus {
    pub fn embedded() -> Self {
        Self::from_documents(
            CORPUS
                .iter()
                .map(|(path, content)| CorpusDocument {
                    relative_path: (*path).to_string(),
                    content: normalise_newlines(content),
                })
                .collect(),
        )
    }

    /// Sorted by path, so the hash does not depend on listing order.
    pub fn from_documents(mut documents: Vec<CorpusDocument>) -> Self {
        documents.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        let mut digest_input = String::new();
        for document in &documents {
            digest_input.push_str(&document.relative_path);
            digest_input.push('\n');
            digest_input.push_str(&sha256_bytes(document.content.as_bytes()));
            digest_input.push('\n');
        }
        Self {
            sha256: sha256_bytes(digest_input.as_bytes()),
            documents,
        }
    }

    pub fn get(&self, relative_path: &str) -> Option<&CorpusDocument> {
        self.documents
            .iter()
            .find(|document| document.relative_path == relative_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_embedded_suite_parses_with_one_case_per_task_kind() {
        let suite = Suite::embedded().unwrap();
        assert_eq!(suite.cases.len(), 6);
        let tasks =
            |wanted: fn(&SuiteCase) -> bool| suite.cases.iter().filter(|c| wanted(c)).count();
        assert_eq!(tasks(|c| matches!(c, SuiteCase::Retrieval { .. })), 3);
        assert_eq!(tasks(|c| matches!(c, SuiteCase::Summary { .. })), 1);
        assert_eq!(tasks(|c| matches!(c, SuiteCase::Interpretation { .. })), 1);
        assert_eq!(tasks(|c| matches!(c, SuiteCase::Edit { .. })), 1);
        assert!(!suite.reference().frozen);
    }

    #[test]
    fn a_duplicate_case_id_is_refused() {
        let json = r#"[
            {"id":"a","task":"summary","input":"x","document":"d.md","requiredFacts":[],"requireSources":true},
            {"id":"a","task":"summary","input":"x","document":"d.md","requiredFacts":[],"requireSources":true}
        ]"#;
        assert!(Suite::from_json(json).is_err());
    }

    #[test]
    fn hashes_are_pinned_so_a_changed_fixture_is_a_deliberate_decision() {
        assert_eq!(
            Suite::embedded().unwrap().sha256,
            "16b6d9fbe4aef9d187c4d36112332c2169943e4a37a535a81f8b78f5d05e11bb"
        );
        assert_eq!(
            Corpus::embedded().sha256,
            "c489bc335fda45486572ceb91e8e84c5fa226e22ab4d2f40028b7a3170b14937"
        );
    }

    #[test]
    fn the_corpus_hash_ignores_listing_order_and_converted_line_endings() {
        let corpus = Corpus::embedded();
        let mut reversed = corpus.documents.clone();
        reversed.reverse();
        assert_eq!(Corpus::from_documents(reversed).sha256, corpus.sha256);

        let converted: Vec<CorpusDocument> = corpus
            .documents
            .iter()
            .map(|document| CorpusDocument {
                relative_path: document.relative_path.clone(),
                content: document.content.replace('\n', "\r\n"),
            })
            .collect();
        let json = SUITE_JSON.replace('\n', "\r\n");
        assert_eq!(
            Suite::from_json(&json).unwrap().sha256,
            Suite::embedded().unwrap().sha256
        );
        let normalised = Corpus::from_documents(
            converted
                .into_iter()
                .map(|document| CorpusDocument {
                    content: normalise_newlines(&document.content),
                    ..document
                })
                .collect(),
        );
        assert_eq!(normalised.sha256, corpus.sha256);
    }

    #[test]
    fn the_corpus_covers_every_text_fixture_on_disk() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/documents");
        let mut on_disk = Vec::new();
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("md") | Some("txt")
                ) {
                    let relative = path.strip_prefix(&root).unwrap();
                    on_disk.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        on_disk.sort();
        let embedded: Vec<String> = Corpus::embedded()
            .documents
            .into_iter()
            .map(|document| document.relative_path)
            .collect();
        assert_eq!(embedded, on_disk);
    }

    #[test]
    fn every_document_a_case_names_is_in_the_corpus() {
        let corpus = Corpus::embedded();
        let suite = Suite::embedded().unwrap();
        let mut named: Vec<&str> = Vec::new();
        for case in &suite.cases {
            match case {
                SuiteCase::Retrieval {
                    relevant_documents,
                    negative_documents,
                    ..
                } => named.extend(
                    relevant_documents
                        .iter()
                        .chain(negative_documents)
                        .map(String::as_str),
                ),
                SuiteCase::Summary { document, .. } => named.push(document),
                SuiteCase::Interpretation {
                    expected_target, ..
                } => named.push(expected_target),
                SuiteCase::Edit {
                    expected_target,
                    review_candidates,
                    unrelated_same_date,
                    ..
                } => {
                    named.push(expected_target);
                    named.extend(
                        review_candidates
                            .iter()
                            .chain(unrelated_same_date)
                            .map(String::as_str),
                    );
                }
            }
        }
        for path in named {
            assert!(corpus.get(path).is_some(), "{path} is not in the corpus");
        }
    }
}
