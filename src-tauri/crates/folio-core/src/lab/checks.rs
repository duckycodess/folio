//! Deterministic checks of a recorded outcome against the suite's labels.
//!
//! Nothing here asks a model whether an answer is good. A check is either a
//! label comparison (`Some(true)` or `Some(false)`) or not evaluated (`None`).
//! A summary never receives a `correctness` verdict: its checks inform a human
//! reviewer and nothing more.

use crate::contracts::{
    GroundedResult, InterpretationResult, OperationProposal, SearchResult, SourcePassage,
};
use crate::lab::record::{Check, OutcomeKind};
use crate::lab::suite::SuiteCase;

/// Retrieval is judged on the top five results.
pub const RETRIEVAL_LIMIT: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct Evaluation {
    /// How the case ended. A failure is `false` (or `None` for a summary), never
    /// silently the same as "not graded".
    pub outcome: OutcomeKind,
    /// `None` for a summary, or when nothing could be judged.
    pub correctness: Option<bool>,
    pub checks: Vec<Check>,
}

fn check(name: impl Into<String>, passed: Option<bool>, detail: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        passed,
        detail: detail.into(),
    }
}

fn all_passed(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.passed == Some(true))
}

fn wrong_case(task: &str, case: &SuiteCase) -> Evaluation {
    Evaluation {
        outcome: OutcomeKind::Valid,
        correctness: None,
        checks: vec![check(
            "caseMatchesTask",
            Some(false),
            format!("case {} is not a {task} case", case.id()),
        )],
    }
}

/// The paths of the top `RETRIEVAL_LIMIT` distinct contents, as search limits
/// them: a byte-identical copy is listed with its original and doesn't take one
/// of the five places.
fn top_distinct(results: &[SearchResult]) -> Vec<&str> {
    let mut seen = std::collections::HashSet::new();
    let mut distinct = 0_usize;
    let mut top = Vec::new();
    for result in results {
        let identity = result.document.content_hash.as_deref().or_else(|| {
            result
                .passages
                .first()
                .map(|passage| passage.document_content_hash.as_str())
        });
        if !identity.is_some_and(|hash| seen.contains(hash)) {
            if distinct >= RETRIEVAL_LIMIT {
                continue;
            }
            distinct += 1;
            if let Some(hash) = identity {
                seen.insert(hash);
            }
        }
        top.push(result.document.relative_path.as_str());
    }
    top
}

/// `results` are the hybrid results in rank order; document ids are the
/// corpus-relative paths.
pub fn check_retrieval(case: &SuiteCase, results: &[SearchResult]) -> Evaluation {
    let SuiteCase::Retrieval {
        relevant_documents,
        negative_documents,
        ..
    } = case
    else {
        return wrong_case("retrieval", case);
    };
    let top = top_distinct(results);
    let found = relevant_documents
        .iter()
        .filter(|path| top.contains(&path.as_str()))
        .count();
    let missing: Vec<&str> = relevant_documents
        .iter()
        .map(String::as_str)
        .filter(|path| !top.contains(path))
        .collect();

    let mut checks = vec![
        check(
            "allRelevantInTop5",
            Some(missing.is_empty()),
            if missing.is_empty() {
                "every labelled document is in the top 5".to_string()
            } else {
                format!("missing from the top 5: {}", missing.join(", "))
            },
        ),
        check(
            "recallAt5",
            None,
            format!(
                "{found}/{} labelled documents in the top 5 (informational)",
                relevant_documents.len()
            ),
        ),
    ];
    if !negative_documents.is_empty() {
        let present: Vec<&str> = negative_documents
            .iter()
            .map(String::as_str)
            .filter(|path| top.contains(path))
            .collect();
        checks.push(check(
            "noNegativeInTop5",
            Some(present.is_empty()),
            if present.is_empty() {
                "no labelled negative is in the top 5".to_string()
            } else {
                format!("labelled negatives in the top 5: {}", present.join(", "))
            },
        ));
    }
    Evaluation {
        outcome: OutcomeKind::Valid,
        correctness: Some(all_passed(
            &checks
                .iter()
                .filter(|c| c.passed.is_some())
                .cloned()
                .collect::<Vec<_>>(),
        )),
        checks,
    }
}

fn operation_name(proposal: &OperationProposal) -> &'static str {
    match proposal {
        OperationProposal::Edit { .. } => "edit",
        OperationProposal::Rename { .. } => "rename",
        OperationProposal::Move { .. } => "move",
        OperationProposal::Create { .. } => "create",
    }
}

fn status_name(result: &InterpretationResult) -> &'static str {
    match result {
        InterpretationResult::Proposal { .. } => "proposal",
        InterpretationResult::NeedsFileSelection { .. } => "needsFileSelection",
        InterpretationResult::NeedsClarification { .. } => "needsClarification",
        InterpretationResult::NonMutating { .. } => "nonMutating",
        InterpretationResult::Unsupported { .. } => "unsupported",
        InterpretationResult::InvalidModelOutput { .. } => "invalidModelOutput",
    }
}

pub fn check_interpretation(case: &SuiteCase, result: &InterpretationResult) -> Evaluation {
    let SuiteCase::Interpretation {
        expected_operation,
        expected_target,
        expected_before,
        expected_after,
        ..
    } = case
    else {
        return wrong_case("interpretation", case);
    };
    let checks = match result {
        InterpretationResult::Proposal { proposal, .. } => {
            let operation = operation_name(proposal);
            let (target, find, replace) = match proposal {
                OperationProposal::Edit {
                    relative_path,
                    find,
                    replace,
                    ..
                } => (Some(relative_path.as_str()), Some(find), Some(replace)),
                OperationProposal::Rename { relative_path, .. }
                | OperationProposal::Move { relative_path, .. } => {
                    (Some(relative_path.as_str()), None, None)
                }
                OperationProposal::Create { .. } => (None, None, None),
            };
            vec![
                check(
                    "operation",
                    Some(operation == expected_operation),
                    format!("expected {expected_operation}, got {operation}"),
                ),
                check(
                    "target",
                    Some(target == Some(expected_target.as_str())),
                    format!(
                        "expected {expected_target}, got {}",
                        target.unwrap_or("none")
                    ),
                ),
                check(
                    "find",
                    Some(find.map(String::as_str) == Some(expected_before.as_str())),
                    format!(
                        "expected {expected_before:?}, got {:?}",
                        find.map(String::as_str)
                    ),
                ),
                check(
                    "replace",
                    Some(replace.map(String::as_str) == Some(expected_after.as_str())),
                    format!(
                        "expected {expected_after:?}, got {:?}",
                        replace.map(String::as_str)
                    ),
                ),
            ]
        }
        other => {
            let detail = format!("no proposal: the result was {}", status_name(other));
            ["operation", "target", "find", "replace"]
                .iter()
                .map(|name| check(*name, Some(false), detail.clone()))
                .collect()
        }
    };
    Evaluation {
        outcome: outcome_of(result),
        correctness: Some(all_passed(&checks)),
        checks,
    }
}

/// A resolved result the model's output could not produce is an invalid output,
/// not a wrong answer.
fn outcome_of(result: &InterpretationResult) -> OutcomeKind {
    if matches!(result, InterpretationResult::InvalidModelOutput { .. }) {
        OutcomeKind::InvalidModelOutput
    } else {
        OutcomeKind::Valid
    }
}

fn digit_runs(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|run| !run.is_empty())
        .map(str::to_string)
        .collect()
}

/// A required fact is a short English label such as "October 20 deadline", and
/// the summary may be in Filipino. When the label has numbers, the check is
/// that each appears as a whole number in the summary; otherwise it is a
/// case-insensitive substring match. Either way it is a string match only.
fn required_fact_check(fact: &str, text: &str) -> Check {
    let numbers = digit_runs(fact);
    let (passed, detail) = if numbers.is_empty() {
        let found = text.to_lowercase().contains(&fact.to_lowercase());
        (
            found,
            format!("string match only: the label itself is {}", presence(found)),
        )
    } else {
        let present = digit_runs(text);
        let missing: Vec<&String> = numbers.iter().filter(|n| !present.contains(n)).collect();
        (
            missing.is_empty(),
            format!(
                "string match only: numbers {:?} from the label; missing {:?}. Not a judgement that the fact is stated correctly",
                numbers, missing
            ),
        )
    };
    check(format!("requiredFact:{fact}"), Some(passed), detail)
}

fn presence(found: bool) -> &'static str {
    if found {
        "present"
    } else {
        "absent"
    }
}

/// Citation and string checks for a human reviewer. `correctness` is always
/// `None`: valid citations or matching strings do not establish that a summary
/// is right.
pub fn check_summary(
    case: &SuiteCase,
    summary: &GroundedResult,
    supplied: &[SourcePassage],
) -> Evaluation {
    let SuiteCase::Summary {
        required_facts,
        require_sources,
        ..
    } = case
    else {
        return wrong_case("summary", case);
    };
    let cited = summary
        .sentences
        .iter()
        .filter(|sentence| !sentence.citations.is_empty())
        .count();
    let uncited = summary.sentences.len() - cited;
    let outside = summary
        .sentences
        .iter()
        .flat_map(|sentence| &sentence.citations)
        .filter(|citation| !supplied.iter().any(|passage| passage == *citation))
        .count();

    let mut checks = Vec::new();
    if *require_sources {
        checks.push(check(
            "hasCitedSentence",
            Some(cited > 0),
            format!(
                "{cited} of {} sentences carry a citation",
                summary.sentences.len()
            ),
        ));
    }
    checks.push(check(
        "noUncitedSentences",
        Some(uncited == 0 && summary.uncited_sentence_count == 0),
        format!(
            "{uncited} sentences without a citation; the provider counted {}",
            summary.uncited_sentence_count
        ),
    ));
    checks.push(check(
        "citationsWithinSupplied",
        Some(outside == 0),
        format!("{outside} citations are not among the passages supplied"),
    ));
    for fact in required_facts {
        checks.push(required_fact_check(fact, &summary.text));
    }
    Evaluation {
        outcome: OutcomeKind::Valid,
        correctness: None,
        checks,
    }
}

/// The edit case judges the proposal only. `workspace_unchanged` is the result
/// of comparing the disposable copy before and after the request.
pub fn check_edit(
    case: &SuiteCase,
    result: &InterpretationResult,
    workspace_unchanged: bool,
) -> Evaluation {
    let SuiteCase::Edit {
        expected_target,
        expected_after,
        unchanged_related_files,
        ..
    } = case
    else {
        return wrong_case("edit", case);
    };
    let (on_target, replaces) = match result {
        InterpretationResult::Proposal {
            proposal:
                OperationProposal::Edit {
                    relative_path,
                    replace,
                    ..
                },
            ..
        } => (
            (
                relative_path == expected_target,
                format!("expected an edit of {expected_target}, got {relative_path}"),
            ),
            (
                replace.contains(expected_after.as_str()),
                format!("replacement {replace:?}, expected it to contain {expected_after:?}"),
            ),
        ),
        other => {
            let detail = format!("no edit proposal: the result was {}", status_name(other));
            ((false, detail.clone()), (false, detail))
        }
    };
    let mut checks = vec![
        check("editProposalOnTarget", Some(on_target.0), on_target.1),
        check("replacementHasNewValue", Some(replaces.0), replaces.1),
    ];
    if *unchanged_related_files {
        checks.push(check(
            "noWritesToDisposableCopy",
            Some(workspace_unchanged),
            "the disposable copy was compared before and after the request",
        ));
    }
    checks.push(check(
        "rippleReviewCandidates",
        None,
        "not run: Ripple needs a scanned index of the disposable copy",
    ));
    let judged: Vec<Check> = checks
        .iter()
        .filter(|c| c.passed.is_some())
        .cloned()
        .collect();
    Evaluation {
        outcome: outcome_of(result),
        correctness: Some(all_passed(&judged)),
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        DocumentRecord, GroundedAnswerKind, GroundedSentence, Language, OffsetUnit, SearchMethod,
    };
    use crate::lab::suite::Suite;

    fn case(id: &str) -> SuiteCase {
        Suite::embedded()
            .unwrap()
            .cases
            .into_iter()
            .find(|case| case.id() == id)
            .unwrap()
    }

    fn hit(path: &str) -> SearchResult {
        SearchResult {
            document: DocumentRecord {
                id: path.into(),
                workspace_id: "lab".into(),
                relative_path: path.into(),
                name: path.into(),
                title: path.into(),
                language: Language::En,
                media_type: "text/markdown".into(),
                size_bytes: 0,
                modified_at_ms: None,
                content: None,
                content_hash: None,
            },
            passages: vec![],
            score: 1.0,
            method: SearchMethod::Hybrid,
            space_fingerprint: None,
        }
    }

    fn passed(evaluation: &Evaluation, name: &str) -> Option<bool> {
        evaluation
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no check {name}"))
            .passed
    }

    #[test]
    fn retrieval_passes_when_every_labelled_document_is_in_the_top_five() {
        let results: Vec<SearchResult> = [
            "projects/project-plan.md",
            "projects/submission-checklist.md",
            "meetings/meeting-notes.md",
            "notes/tala-sa-proyekto.md",
        ]
        .iter()
        .map(|path| hit(path))
        .collect();
        let evaluation = check_retrieval(&case("retrieval-en"), &results);
        assert_eq!(evaluation.correctness, Some(true));
        assert_eq!(passed(&evaluation, "recallAt5"), None);
    }

    #[test]
    fn retrieval_fails_on_a_missing_relevant_document_or_a_negative() {
        let mut results: Vec<SearchResult> = [
            "projects/project-plan.md",
            "projects/submission-checklist.md",
            "meetings/meeting-notes.md",
        ]
        .iter()
        .map(|path| hit(path))
        .collect();
        let evaluation = check_retrieval(&case("retrieval-en"), &results);
        assert_eq!(evaluation.correctness, Some(false));
        assert_eq!(passed(&evaluation, "allRelevantInTop5"), Some(false));

        results.push(hit("notes/tala-sa-proyekto.md"));
        results.push(hit("personal/grocery-list.md"));
        let evaluation = check_retrieval(&case("retrieval-en"), &results);
        assert_eq!(passed(&evaluation, "allRelevantInTop5"), Some(true));
        assert_eq!(passed(&evaluation, "noNegativeInTop5"), Some(false));
        assert_eq!(evaluation.correctness, Some(false));
    }

    #[test]
    fn a_relevant_document_below_rank_five_does_not_count() {
        let mut results: Vec<SearchResult> = (0..5)
            .map(|index| hit(&format!("filler/{index}.md")))
            .collect();
        results.push(hit("projects/project-plan.md"));
        let evaluation = check_retrieval(&case("retrieval-fil"), &results);
        assert_eq!(evaluation.correctness, Some(false));
    }

    #[test]
    fn an_identical_copy_does_not_take_one_of_the_five_places() {
        let ranked = |paths: &[&str]| -> Vec<SearchResult> {
            paths
                .iter()
                .map(|path| {
                    let mut result = hit(path);
                    // The copy has the same bytes, so the same content hash.
                    if path.contains("project-plan") {
                        result.document.content_hash = Some("sha256:plan".into());
                    }
                    result
                })
                .collect()
        };
        // Six entries, five distinct contents: the sixth entry is in the top 5.
        let results = ranked(&[
            "projects/project-plan.md",
            "archive/project-plan-copy.md",
            "projects/submission-checklist.md",
            "meetings/meeting-notes.md",
            "filler/0.md",
            "notes/tala-sa-proyekto.md",
        ]);
        let evaluation = check_retrieval(&case("retrieval-en"), &results);
        assert_eq!(passed(&evaluation, "allRelevantInTop5"), Some(true));

        // A negative at the fifth distinct place is caught.
        let results = ranked(&[
            "projects/project-plan.md",
            "archive/project-plan-copy.md",
            "projects/submission-checklist.md",
            "meetings/meeting-notes.md",
            "notes/tala-sa-proyekto.md",
            "personal/grocery-list.md",
        ]);
        let evaluation = check_retrieval(&case("retrieval-en"), &results);
        assert_eq!(passed(&evaluation, "noNegativeInTop5"), Some(false));
    }

    #[test]
    fn retrieval_without_labelled_negatives_has_no_negative_check() {
        let results: Vec<SearchResult> = [
            "projects/project-plan.md",
            "notes/tala-sa-proyekto.md",
            "projects/submission-checklist.md",
        ]
        .iter()
        .map(|path| hit(path))
        .collect();
        let evaluation = check_retrieval(&case("retrieval-fil"), &results);
        assert!(evaluation
            .checks
            .iter()
            .all(|c| c.name != "noNegativeInTop5"));
        assert_eq!(evaluation.correctness, Some(true));
    }

    fn edit_proposal(path: &str, find: &str, replace: &str) -> InterpretationResult {
        InterpretationResult::Proposal {
            proposal: OperationProposal::Edit {
                document_id: path.into(),
                relative_path: path.into(),
                observed_content_hash: "sha256:x".into(),
                find: find.into(),
                replace: replace.into(),
                target_evidence: SourcePassage {
                    document_id: path.into(),
                    document_content_hash: "sha256:x".into(),
                    offset_unit: OffsetUnit::Utf8Byte,
                    start: 0,
                    end: 1,
                    text: "x".into(),
                    page: None,
                },
            },
            request_language: Language::Mixed,
            exact_duplicate_paths: vec![],
        }
    }

    #[test]
    fn interpretation_requires_the_exact_operation_target_find_and_replace() {
        let case = case("action-taglish");
        let good = edit_proposal("projects/project-plan.md", "October 20", "October 23");
        let evaluation = check_interpretation(&case, &good);
        assert_eq!(evaluation.correctness, Some(true));

        // The failure the Linux diagnostics saw: both fields hold the whole phrase.
        let swapped = edit_proposal(
            "projects/project-plan.md",
            "October 20 to October 23",
            "October 20 to October 23",
        );
        let evaluation = check_interpretation(&case, &swapped);
        assert_eq!(evaluation.correctness, Some(false));
        assert_eq!(passed(&evaluation, "operation"), Some(true));
        assert_eq!(passed(&evaluation, "find"), Some(false));
        assert_eq!(passed(&evaluation, "replace"), Some(false));

        let wrong_file = edit_proposal("archive/project-plan-copy.md", "October 20", "October 23");
        assert_eq!(
            check_interpretation(&case, &wrong_file).correctness,
            Some(false)
        );
    }

    #[test]
    fn interpretation_that_asks_a_question_or_is_invalid_is_not_correct() {
        let case = case("action-taglish");
        for result in [
            InterpretationResult::NeedsClarification {
                question: "Which?".into(),
                reason: "ambiguous".into(),
            },
            InterpretationResult::InvalidModelOutput {
                raw_output_digest: "sha256:x".into(),
            },
        ] {
            let evaluation = check_interpretation(&case, &result);
            assert_eq!(evaluation.correctness, Some(false));
            assert_eq!(passed(&evaluation, "operation"), Some(false));
        }
    }

    fn passage(text: &str) -> SourcePassage {
        SourcePassage {
            document_id: "projects/project-plan.md".into(),
            document_content_hash: "sha256:x".into(),
            offset_unit: OffsetUnit::Utf8Byte,
            start: 0,
            end: text.len(),
            text: text.into(),
            page: None,
        }
    }

    fn summary(text: &str, sentences: Vec<GroundedSentence>, uncited: u32) -> GroundedResult {
        GroundedResult {
            text: text.into(),
            sources: vec![],
            coverage: vec![],
            model_id: "m".into(),
            revision: "r".into(),
            kind: GroundedAnswerKind::FileSummary,
            sentences,
            coverage_ranges: vec![],
            uncited_sentence_count: uncited,
        }
    }

    #[test]
    fn a_summary_is_never_given_a_correctness_verdict() {
        let supplied = vec![passage("Deadline: October 20")];
        let good = summary(
            "Ang deadline ay Oktubre 20, 12 estudyante, presentasyon Oktubre 24.",
            vec![GroundedSentence {
                text: "Ang deadline ay Oktubre 20.".into(),
                citations: supplied.clone(),
            }],
            0,
        );
        let evaluation = check_summary(&case("summary-fil"), &good, &supplied);
        assert_eq!(evaluation.correctness, None);
        assert_eq!(passed(&evaluation, "hasCitedSentence"), Some(true));
        assert_eq!(passed(&evaluation, "noUncitedSentences"), Some(true));
        assert_eq!(passed(&evaluation, "citationsWithinSupplied"), Some(true));
        assert_eq!(
            passed(&evaluation, "requiredFact:12 volunteer students"),
            Some(true)
        );

        let bad = summary("Walang detalye.", vec![], 0);
        let evaluation = check_summary(&case("summary-fil"), &bad, &supplied);
        assert_eq!(evaluation.correctness, None);
        assert_eq!(passed(&evaluation, "hasCitedSentence"), Some(false));
        assert_eq!(
            passed(&evaluation, "requiredFact:October 20 deadline"),
            Some(false)
        );
    }

    #[test]
    fn summary_citations_are_checked_against_the_supplied_passages() {
        let supplied = vec![passage("Deadline: October 20")];
        let outside = summary(
            "Oktubre 20.",
            vec![GroundedSentence {
                text: "Oktubre 20.".into(),
                citations: vec![passage("something never supplied")],
            }],
            0,
        );
        let evaluation = check_summary(&case("summary-fil"), &outside, &supplied);
        assert_eq!(passed(&evaluation, "citationsWithinSupplied"), Some(false));

        let uncited = summary(
            "Oktubre 20. See the checklist.",
            vec![
                GroundedSentence {
                    text: "Oktubre 20.".into(),
                    citations: supplied.clone(),
                },
                GroundedSentence {
                    text: "See the checklist.".into(),
                    citations: vec![],
                },
            ],
            1,
        );
        let evaluation = check_summary(&case("summary-fil"), &uncited, &supplied);
        assert_eq!(passed(&evaluation, "noUncitedSentences"), Some(false));
    }

    #[test]
    fn a_required_number_must_be_a_whole_number_in_the_summary() {
        let fact = required_fact_check("12 volunteer students", "May 120 estudyante.");
        assert_eq!(fact.passed, Some(false));
        let fact = required_fact_check("12 volunteer students", "May 12 estudyante.");
        assert_eq!(fact.passed, Some(true));
        assert!(fact.detail.contains("string match only"));
        let fact = required_fact_check("Consent form", "the CONSENT FORM is ready");
        assert_eq!(fact.passed, Some(true));
    }

    #[test]
    fn the_edit_case_judges_the_proposal_and_leaves_ripple_unrun() {
        let case = case("ripple-deadline");
        let good = edit_proposal("projects/project-plan.md", "October 20", "October 23");
        let evaluation = check_edit(&case, &good, true);
        assert_eq!(evaluation.correctness, Some(true));
        assert_eq!(passed(&evaluation, "rippleReviewCandidates"), None);

        let evaluation = check_edit(&case, &good, false);
        assert_eq!(evaluation.correctness, Some(false));
        assert_eq!(passed(&evaluation, "noWritesToDisposableCopy"), Some(false));

        let wrong_date = edit_proposal("projects/project-plan.md", "October 20", "October 24");
        assert_eq!(
            check_edit(&case, &wrong_date, true).correctness,
            Some(false)
        );
        let wrong_file = edit_proposal("courses/math-review.md", "October 20", "October 23");
        assert_eq!(
            check_edit(&case, &wrong_file, true).correctness,
            Some(false)
        );
    }

    #[test]
    fn an_invalid_model_output_is_its_own_outcome_and_never_correct() {
        let invalid = InterpretationResult::InvalidModelOutput {
            raw_output_digest: "sha256:x".into(),
        };
        for evaluation in [
            check_interpretation(&case("action-taglish"), &invalid),
            check_edit(&case("ripple-deadline"), &invalid, true),
        ] {
            assert_eq!(evaluation.outcome, OutcomeKind::InvalidModelOutput);
            assert_eq!(evaluation.correctness, Some(false));
        }
        let good = edit_proposal("projects/project-plan.md", "October 20", "October 23");
        assert_eq!(
            check_interpretation(&case("action-taglish"), &good).outcome,
            OutcomeKind::Valid
        );
    }

    #[test]
    fn a_check_for_the_wrong_kind_of_case_is_a_failure_not_a_pass() {
        let evaluation = check_retrieval(&case("action-taglish"), &[]);
        assert_eq!(evaluation.correctness, None);
        assert_eq!(passed(&evaluation, "caseMatchesTask"), Some(false));
    }
}
