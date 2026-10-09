//! Cross-language contract checks.
//!
//! `fixtures/contracts/contract-cases.json` is produced by
//! `fixtures/contracts/generate-contract-cases.py`, a third implementation
//! written from `docs/contracts.md` rather than from this crate or the
//! TypeScript one, and the TypeScript suite reads the same file. Both
//! languages are therefore pinned to one wire encoding rather than to each
//! other's bugs.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::Value;

    use crate::contracts::{ActionPlan, ActivityBatch, PlanSource};
    use crate::error::{ErrorCode, ALL_ERROR_CODES};
    use crate::identity::{
        assert_portable_destination, content_hash, document_id, embedding_space_fingerprint,
        media_type_for_path, normalize_relative_path, workspace_id_for,
    };
    use crate::plan::{canonical_plan_bytes, plan_digest};

    fn cases() -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("fixtures")
            .join("contracts")
            .join("contract-cases.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing contract fixtures at {}", path.display()));
        serde_json::from_str(&text).expect("contract fixtures are valid JSON")
    }

    fn code_of(result: Result<String, crate::error::FolioError>) -> String {
        match result {
            Ok(_) => "no-error".to_string(),
            Err(failure) => serde_json::to_value(failure.code)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string(),
        }
    }

    #[test]
    fn agrees_on_the_frozen_error_codes() {
        let cases = cases();
        let expected: Vec<String> = cases["errorCodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect();
        let actual: Vec<String> = ALL_ERROR_CODES
            .iter()
            .map(|code| {
                serde_json::to_value(code)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(actual, expected);
        // A code added to the enum but not to the list would not compile.
        assert_eq!(ALL_ERROR_CODES.len(), expected.len());
        assert_eq!(
            serde_json::to_value(ErrorCode::WriterNotImplemented).unwrap(),
            serde_json::json!("writerNotImplemented")
        );
    }

    #[test]
    fn agrees_on_content_hashes() {
        for entry in cases()["hashes"].as_array().unwrap() {
            let text = entry["text"].as_str().unwrap();
            assert_eq!(
                content_hash(text.as_bytes()),
                entry["expected"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn agrees_on_workspace_identity() {
        for entry in cases()["identity"]["workspaceId"].as_array().unwrap() {
            let path = Path::new(entry["canonicalRootPath"].as_str().unwrap());
            assert_eq!(
                workspace_id_for(path).unwrap(),
                entry["expected"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn agrees_on_accepted_and_rejected_relative_paths() {
        let cases = cases();
        for entry in cases["identity"]["normalizeRelativePath"]["accepted"]
            .as_array()
            .unwrap()
        {
            assert_eq!(
                normalize_relative_path(entry["input"].as_str().unwrap()).unwrap(),
                entry["expected"].as_str().unwrap(),
            );
        }
        for entry in cases["identity"]["normalizeRelativePath"]["rejected"]
            .as_array()
            .unwrap()
        {
            let input = entry["input"].as_str().unwrap();
            assert_eq!(
                code_of(normalize_relative_path(input)),
                entry["code"].as_str().unwrap(),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn agrees_on_document_identity_including_decomposed_filenames() {
        for entry in cases()["identity"]["documentId"].as_array().unwrap() {
            let workspace = entry["workspaceId"].as_str().unwrap();
            let relative =
                normalize_relative_path(entry["relativePath"].as_str().unwrap()).unwrap();
            assert_eq!(
                document_id(workspace, &relative),
                entry["expected"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn agrees_on_destinations_no_platform_can_store() {
        for entry in cases()["identity"]["portableDestination"]["rejected"]
            .as_array()
            .unwrap()
        {
            let input = entry["input"].as_str().unwrap();
            assert_eq!(
                code_of(assert_portable_destination(input)),
                entry["code"].as_str().unwrap(),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn agrees_on_media_types() {
        for entry in cases()["identity"]["mediaType"].as_array().unwrap() {
            let path = entry["path"].as_str().unwrap();
            let expected = entry["expected"].as_str();
            assert_eq!(media_type_for_path(path), expected, "path {path:?}");
        }
    }

    #[test]
    fn agrees_on_embedding_space_fingerprints() {
        for entry in cases()["identity"]["embeddingSpaceFingerprint"]
            .as_array()
            .unwrap()
        {
            let space = &entry["space"];
            assert_eq!(
                embedding_space_fingerprint(
                    space["modelId"].as_str().unwrap(),
                    space["revision"].as_str().unwrap(),
                    space["quantization"].as_str().unwrap(),
                    space["dimensions"].as_u64().unwrap() as u32,
                    space["preprocessingFingerprint"].as_str().unwrap(),
                ),
                entry["expected"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn agrees_on_utf8_source_offsets() {
        let cases = cases();
        assert_eq!(cases["offsetUnit"].as_str().unwrap(), "utf8Byte");
        for entry in cases["offsets"]["cases"].as_array().unwrap() {
            let text = entry["text"].as_str().unwrap();
            assert_eq!(
                text.as_bytes().len() as u64,
                entry["utf8Length"].as_u64().unwrap()
            );
            assert_eq!(
                text.encode_utf16().count() as u64,
                entry["utf16Length"].as_u64().unwrap()
            );
            let start = entry["passage"]["start"].as_u64().unwrap() as usize;
            let end = entry["passage"]["end"].as_u64().unwrap() as usize;
            assert_eq!(
                &text[start..end],
                entry["passage"]["text"].as_str().unwrap()
            );
            assert_eq!(
                content_hash(text.as_bytes()),
                entry["contentHash"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn treats_an_offset_inside_a_character_as_invalid() {
        for entry in cases()["offsets"]["invalid"].as_array().unwrap() {
            let text = entry["text"].as_str().unwrap();
            let start = entry["start"].as_u64().unwrap() as usize;
            assert!(
                !text.is_char_boundary(start),
                "expected {start} to fall inside a character"
            );
        }
    }

    #[test]
    fn agrees_on_canonical_plan_bytes_and_digests() {
        let mut kinds = std::collections::BTreeSet::new();
        for entry in cases()["plans"].as_array().unwrap() {
            let plan: ActionPlan = serde_json::from_value(entry["plan"].clone())
                .expect("the fixture plan deserializes into the native contract");
            kinds.extend(plan.operations.iter().map(|operation| operation.kind()));
            let canonical = canonical_plan_bytes(&plan);
            assert_eq!(
                String::from_utf8(canonical.clone()).unwrap(),
                entry["canonical"].as_str().unwrap()
            );
            assert_eq!(
                canonical.len() as u64,
                entry["canonicalByteLength"].as_u64().unwrap()
            );
            assert_eq!(plan_digest(&plan), entry["digest"].as_str().unwrap());
            assert_eq!(plan.digest, entry["digest"].as_str().unwrap());
        }
        assert_eq!(
            kinds,
            ["create", "delete", "edit", "rename"].into_iter().collect(),
            "the fixtures cover every distinct canonical layout"
        );
    }
    #[test]
    fn agrees_on_the_plan_sources_a_new_plan_can_name() {
        let cases = cases();
        let sources = cases["planSources"].as_array().unwrap();
        for value in sources {
            let source: PlanSource = serde_json::from_value(value.clone())
                .expect("each plan source deserializes into the native enum");
            assert_ne!(source, PlanSource::Unknown);
            assert_eq!(source.as_str(), value.as_str().unwrap());
            assert_eq!(PlanSource::from_stored(source.as_str()), source);
        }
        assert!(!sources.contains(&serde_json::json!("unknown")));
    }

    #[test]
    fn agrees_on_the_activity_wire_shape() {
        for entry in cases()["activity"].as_array().unwrap() {
            let label = entry["label"].as_str().unwrap();
            let batch: ActivityBatch = serde_json::from_value(entry["batch"].clone())
                .unwrap_or_else(|failure| panic!("{label}: {failure}"));
            // Serialized again, an absent field stays absent: no nulls, no extras.
            assert_eq!(serde_json::to_value(&batch).unwrap(), entry["batch"], "{label}");
        }
    }

    #[test]
    fn agrees_on_the_ai_relationship_wire_shapes() {
        use crate::contracts::SourcePassage;
        use crate::index::{Relationship, SharedFactCandidateRelationship, SimilarityRelationship};

        let cases = cases();
        let ai = &cases["aiRelationships"];
        let passages = |value: &Value| -> Vec<SourcePassage> {
            serde_json::from_value(value.clone()).expect("fixture passages")
        };
        let text = |value: &Value| value.as_str().unwrap().to_owned();

        let similarity = &ai["similarity"];
        let native = Relationship::Similarity(SimilarityRelationship {
            source_id: text(&similarity["sourceId"]),
            target_id: text(&similarity["targetId"]),
            source_content_hash: text(&similarity["sourceContentHash"]),
            target_content_hash: text(&similarity["targetContentHash"]),
            relationship_type: "similarity",
            provenance: "embedding",
            space_fingerprint: text(&similarity["spaceFingerprint"]),
            score: similarity["score"].as_f64().unwrap() as f32,
            source_evidence: passages(&similarity["sourceEvidence"]),
            target_evidence: passages(&similarity["targetEvidence"]),
        });
        assert_eq!(serde_json::to_value(&native).unwrap(), *similarity);

        let shared = &ai["sharedFactCandidate"];
        let native = Relationship::SharedFactCandidate(SharedFactCandidateRelationship {
            source_id: text(&shared["sourceId"]),
            target_id: text(&shared["targetId"]),
            source_content_hash: text(&shared["sourceContentHash"]),
            target_content_hash: text(&shared["targetContentHash"]),
            relationship_type: "sharedFactCandidate",
            provenance: "embedding",
            source_evidence: passages(&shared["sourceEvidence"]),
            target_evidence: passages(&shared["targetEvidence"]),
            confidence: None,
        });
        // A candidate carries no confidence: the key is absent, not null.
        assert_eq!(serde_json::to_value(&native).unwrap(), *shared);
        assert!(shared.get("confidence").is_none());
    }

    #[test]
    fn agrees_on_the_ai_coverage_and_refresh_wire_shapes() {
        use crate::ai_discovery::{CoverageState, RelationshipCoverage, RunEnd};

        let cases = cases();
        let ai = &cases["aiRelationships"];
        let state_name = |state: CoverageState| {
            serde_json::to_value(state).unwrap().as_str().unwrap().to_owned()
        };
        let states: Vec<String> = [
            CoverageState::NoActiveSpace,
            CoverageState::EmbeddingIncomplete,
            CoverageState::Partial,
            CoverageState::Complete,
        ]
        .into_iter()
        .map(state_name)
        .collect();
        assert_eq!(serde_json::to_value(&states).unwrap(), ai["coverageStates"]);

        let ends: Vec<String> = [
            RunEnd::Complete,
            RunEnd::BudgetExhausted,
            RunEnd::Cancelled,
            RunEnd::SpaceChanged,
        ]
        .into_iter()
        .map(|end| serde_json::to_value(end).unwrap().as_str().unwrap().to_owned())
        .collect();
        assert_eq!(serde_json::to_value(&ends).unwrap(), ai["refreshEnds"]);

        for entry in ai["coverage"].as_array().unwrap() {
            let state = match entry["state"].as_str().unwrap() {
                "noActiveSpace" => CoverageState::NoActiveSpace,
                "embeddingIncomplete" => CoverageState::EmbeddingIncomplete,
                "partial" => CoverageState::Partial,
                _ => CoverageState::Complete,
            };
            let native = RelationshipCoverage {
                state,
                space_fingerprint: entry.get("spaceFingerprint").map(|value| value.as_str().unwrap().to_owned()),
                eligible_documents: entry["eligibleDocuments"].as_u64().unwrap() as usize,
                indexed_documents: entry["indexedDocuments"].as_u64().unwrap() as usize,
                pairs_considered: entry["pairsConsidered"].as_u64().unwrap(),
                pairs_remaining: entry["pairsRemaining"].as_u64().unwrap(),
                overflow_documents: entry["overflowDocuments"].as_u64().unwrap() as usize,
            };
            assert_eq!(serde_json::to_value(&native).unwrap(), *entry);
        }
    }
}
