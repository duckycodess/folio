import {
  SOURCE_OFFSET_UNIT,
  type ContentHash,
  type DocumentId,
  type Relationship,
  type SourcePassage,
} from "./contracts";
import { folioError } from "./errors";
import { isContentHash } from "./hash";

function assertPassages(
  passages: SourcePassage[],
  documentId: DocumentId,
  contentHash: ContentHash,
  label: string,
): void {
  if (passages.length === 0) {
    throw folioError(
      "evidenceInvalid",
      `A ${label} connection needs at least one source passage.`,
      { documentId },
    );
  }
  for (const passage of passages) {
    if (passage.documentId !== documentId) {
      throw folioError(
        "evidenceInvalid",
        "Evidence must come from the document it is attached to.",
        { expected: documentId, observed: passage.documentId },
      );
    }
    if (passage.offsetUnit !== SOURCE_OFFSET_UNIT) {
      throw folioError(
        "evidenceInvalid",
        `Source offsets must be ${SOURCE_OFFSET_UNIT} offsets.`,
        { documentId },
      );
    }
    if (passage.documentContentHash !== contentHash) {
      throw folioError(
        "evidenceInvalid",
        "Evidence refers to a different revision of the document.",
        { documentId },
      );
    }
    if (
      !Number.isInteger(passage.start) ||
      !Number.isInteger(passage.end) ||
      passage.start < 0 ||
      passage.end <= passage.start
    ) {
      throw folioError("evidenceInvalid", "A source range is not usable.", {
        documentId,
        start: passage.start,
        end: passage.end,
      });
    }
    if (passage.page !== undefined && passage.page < 1) {
      throw folioError("evidenceInvalid", "Page numbers start at 1.", {
        documentId,
        page: passage.page,
      });
    }
  }
}

function assertUnitInterval(value: number, label: string): void {
  if (!Number.isFinite(value) || value < 0 || value > 1) {
    throw folioError("evidenceInvalid", `${label} must be between 0 and 1.`, {
      value,
    });
  }
}

/**
 * Every typed connection carries evidence of its own kind, bound to the
 * document revisions it was read from. Similarity is never presented as proof
 * that an edit must propagate, and a shared fact candidate is never a confirmed
 * contradiction.
 */
export function validateRelationship(relationship: Relationship): void {
  if (relationship.sourceId === relationship.targetId) {
    throw folioError(
      "evidenceInvalid",
      "A connection needs two different documents.",
      { documentId: relationship.sourceId },
    );
  }
  for (const hash of [
    relationship.sourceContentHash,
    relationship.targetContentHash,
  ]) {
    if (!isContentHash(hash)) {
      throw folioError(
        "evidenceInvalid",
        "A connection records the revision of both documents.",
        { documentId: relationship.sourceId },
      );
    }
  }
  switch (relationship.type) {
    case "explicitReference":
      assertPassages(
        relationship.evidence,
        relationship.sourceId,
        relationship.sourceContentHash,
        "reference",
      );
      if (
        !relationship.link.rawTarget ||
        !relationship.link.resolvedRelativePath
      ) {
        throw folioError(
          "evidenceInvalid",
          "An explicit reference records the link it came from.",
          { documentId: relationship.sourceId },
        );
      }
      return;
    case "similarity":
      assertUnitInterval(relationship.score, "A similarity score");
      if (!relationship.spaceFingerprint) {
        throw folioError(
          "embeddingSpaceMismatch",
          "A similarity connection records the embedding space it was computed in.",
          { documentId: relationship.sourceId },
        );
      }
      assertPassages(
        relationship.sourceEvidence,
        relationship.sourceId,
        relationship.sourceContentHash,
        "similarity",
      );
      assertPassages(
        relationship.targetEvidence,
        relationship.targetId,
        relationship.targetContentHash,
        "similarity",
      );
      return;
    case "sharedFactCandidate":
      if (relationship.confidence !== undefined) {
        assertUnitInterval(relationship.confidence, "A confidence");
      }
      assertPassages(
        relationship.sourceEvidence,
        relationship.sourceId,
        relationship.sourceContentHash,
        "shared fact candidate",
      );
      assertPassages(
        relationship.targetEvidence,
        relationship.targetId,
        relationship.targetContentHash,
        "shared fact candidate",
      );
      return;
  }
}

/** Passages a connection offers for inspection, in a single list. */
export function relationshipEvidence(
  relationship: Relationship,
): SourcePassage[] {
  return relationship.type === "explicitReference"
    ? relationship.evidence
    : [...relationship.sourceEvidence, ...relationship.targetEvidence];
}
