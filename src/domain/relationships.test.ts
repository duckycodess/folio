import { describe, expect, it } from "vitest";
import { relationshipEvidence, validateRelationship } from "./relationships";
import { isFolioError } from "./errors";
import { hashText } from "./hash";
import { passageFromUtf16Range } from "./offsets";
import { embeddingSpaceFingerprint } from "./identity";
import type { FolioErrorCode, Relationship, SourcePassage } from "./contracts";

const SOURCE_ID = "w:meetings/meeting-notes.md";
const TARGET_ID = "w:projects/project-plan.md";
const SOURCE_TEXT = "Tinalakay namin ang [plano](../projects/project-plan.md).";
const TARGET_TEXT = "Deadline: October 20";

function codeOf(run: () => unknown): FolioErrorCode | string {
  try {
    run();
  } catch (cause) {
    return isFolioError(cause) ? cause.code : `not-a-folio-error: ${cause}`;
  }
  return "no-error";
}

async function passage(
  documentId: string,
  text: string,
  startIndex: number,
  endIndex: number,
): Promise<SourcePassage> {
  return passageFromUtf16Range({
    documentId,
    documentContentHash: await hashText(text),
    text,
    startIndex,
    endIndex,
  });
}

describe("typed relationship evidence", () => {
  it("accepts an explicit reference carrying the link it came from", async () => {
    const relationship: Relationship = {
      type: "explicitReference",
      provenance: "documentLink",
      sourceId: SOURCE_ID,
      targetId: TARGET_ID,
      sourceContentHash: await hashText(SOURCE_TEXT),
      targetContentHash: await hashText(TARGET_TEXT),
      link: {
        rawTarget: "../projects/project-plan.md",
        resolvedRelativePath: "projects/project-plan.md",
      },
      evidence: [
        await passage(
          SOURCE_ID,
          SOURCE_TEXT,
          SOURCE_TEXT.indexOf("["),
          SOURCE_TEXT.length - 1,
        ),
      ],
    };
    expect(codeOf(() => validateRelationship(relationship))).toBe("no-error");
    expect(relationshipEvidence(relationship)).toHaveLength(1);
  });

  it("refuses a reference whose evidence came from the other document", async () => {
    const relationship: Relationship = {
      type: "explicitReference",
      provenance: "documentLink",
      sourceId: SOURCE_ID,
      targetId: TARGET_ID,
      sourceContentHash: await hashText(SOURCE_TEXT),
      targetContentHash: await hashText(TARGET_TEXT),
      link: {
        rawTarget: "../projects/project-plan.md",
        resolvedRelativePath: "projects/project-plan.md",
      },
      evidence: [await passage(TARGET_ID, TARGET_TEXT, 0, 8)],
    };
    expect(codeOf(() => validateRelationship(relationship))).toBe(
      "evidenceInvalid",
    );
  });

  it("requires a similarity edge to name its embedding space and both passages", async () => {
    const base = {
      type: "similarity" as const,
      provenance: "embedding" as const,
      sourceId: SOURCE_ID,
      targetId: TARGET_ID,
      sourceContentHash: await hashText(SOURCE_TEXT),
      targetContentHash: await hashText(TARGET_TEXT),
      score: 0.82,
      sourceEvidence: [await passage(SOURCE_ID, SOURCE_TEXT, 0, 20)],
      targetEvidence: [await passage(TARGET_ID, TARGET_TEXT, 0, 8)],
      spaceFingerprint: embeddingSpaceFingerprint({
        modelId: "intfloat/multilingual-e5-small",
        revision: "r1",
        quantization: "q8",
        dimensions: 384,
        preprocessingFingerprint: "query-passage-v1",
      }),
    };
    expect(codeOf(() => validateRelationship(base))).toBe("no-error");
    expect(
      codeOf(() => validateRelationship({ ...base, spaceFingerprint: "" })),
    ).toBe("embeddingSpaceMismatch");
    expect(
      codeOf(() => validateRelationship({ ...base, targetEvidence: [] })),
    ).toBe("evidenceInvalid");
    expect(codeOf(() => validateRelationship({ ...base, score: 1.4 }))).toBe(
      "evidenceInvalid",
    );
  });

  it("requires a shared fact candidate to show passages in both documents", async () => {
    const base = {
      type: "sharedFactCandidate" as const,
      provenance: "model" as const,
      sourceId: SOURCE_ID,
      targetId: TARGET_ID,
      sourceContentHash: await hashText(SOURCE_TEXT),
      targetContentHash: await hashText(TARGET_TEXT),
      sourceEvidence: [await passage(SOURCE_ID, SOURCE_TEXT, 0, 20)],
      targetEvidence: [await passage(TARGET_ID, TARGET_TEXT, 0, 8)],
    };
    expect(codeOf(() => validateRelationship(base))).toBe("no-error");
    expect(
      codeOf(() => validateRelationship({ ...base, sourceEvidence: [] })),
    ).toBe("evidenceInvalid");
    expect(relationshipEvidence(base)).toHaveLength(2);
  });

  it("refuses evidence taken from a different revision than the edge records", async () => {
    const relationship: Relationship = {
      type: "explicitReference",
      provenance: "documentLink",
      sourceId: SOURCE_ID,
      targetId: TARGET_ID,
      sourceContentHash: await hashText("Ibang bersyon ng dokumento."),
      targetContentHash: await hashText(TARGET_TEXT),
      link: {
        rawTarget: "../projects/project-plan.md",
        resolvedRelativePath: "projects/project-plan.md",
      },
      evidence: [await passage(SOURCE_ID, SOURCE_TEXT, 0, 20)],
    };
    expect(codeOf(() => validateRelationship(relationship))).toBe(
      "evidenceInvalid",
    );
  });
});
