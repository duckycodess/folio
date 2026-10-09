//! Cross-language contract checks.
//!
//! `fixtures/contracts/contract-cases.json` is produced by an implementation
//! that is neither this one nor the TypeScript one, and the TypeScript suite
//! reads the same file. Both languages are therefore pinned to one wire
//! encoding rather than to each other's bugs.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::Value;

    use crate::contracts::ActionPlan;
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
        for entry in cases()["plans"].as_array().unwrap() {
            let plan: ActionPlan = serde_json::from_value(entry["plan"].clone())
                .expect("the fixture plan deserializes into the native contract");
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
    }
}
