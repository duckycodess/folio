import { describe, expect, it } from "vitest";
import {
  assertPortableDestination,
  assertSameEmbeddingSpace,
  documentIdFor,
  embeddingSpaceFingerprint,
  mediaTypeForPath,
  normalizeRelativePath,
  parseDocumentId,
  workspaceIdForPath,
} from "./identity";
import { isFolioError } from "./errors";
import type { EmbeddingSpace, FolioErrorCode } from "./contracts";

function codeOf(run: () => unknown): FolioErrorCode | string {
  try {
    run();
  } catch (cause) {
    return isFolioError(cause) ? cause.code : `not-a-folio-error: ${cause}`;
  }
  return "no-error";
}

describe("document identity", () => {
  it("refuses paths that could reach outside the authorized folder", () => {
    expect(codeOf(() => normalizeRelativePath("../outside.md"))).toBe(
      "pathEscapesWorkspace",
    );
    expect(codeOf(() => normalizeRelativePath("notes/../../outside.md"))).toBe(
      "pathEscapesWorkspace",
    );
    expect(codeOf(() => normalizeRelativePath("/etc/passwd"))).toBe(
      "pathNotRelative",
    );
    expect(codeOf(() => normalizeRelativePath("C:/Users/x/notes.md"))).toBe(
      "pathNotRelative",
    );
  });

  it("refuses a separator-ambiguous path instead of rewriting it", () => {
    // A Unix file really named 'notes\paalala.md' must not be silently turned
    // into a two-segment path, because that identifies a different file.
    expect(codeOf(() => normalizeRelativePath("notes\\paalala.md"))).toBe(
      "pathNotRelative",
    );
  });

  it("gives a decomposed and a composed Filipino filename the same identity", () => {
    const decomposed = "courses/pagsasanay-n\u0303.md";
    const composed = "courses/pagsasanay-ñ.md";
    expect(normalizeRelativePath(decomposed)).toBe(composed);
    expect(documentIdFor("w".repeat(64), decomposed)).toBe(
      documentIdFor("w".repeat(64), composed),
    );
  });

  it("round-trips an identity without losing the path", () => {
    const workspaceId = "w".repeat(64);
    const relativePath = "notes/tala sa proyekto.md";
    const id = documentIdFor(workspaceId, relativePath);
    expect(parseDocumentId(id)).toEqual({ workspaceId, relativePath });
  });

  it("keeps a workspace identity stable for the same canonical folder", async () => {
    const first = await workspaceIdForPath("/home/mag-aaral/Mga Dokumento");
    const second = await workspaceIdForPath("/home/mag-aaral/Mga Dokumento");
    const other = await workspaceIdForPath("/home/mag-aaral/Ibang Folder");
    expect(first).toBe(second);
    expect(first).not.toBe(other);
    expect(first).toMatch(/^[0-9a-f]{64}$/);
    expect(first).not.toContain(":");
  });

  it("refuses destinations that Windows cannot store", () => {
    expect(codeOf(() => assertPortableDestination("notes/plano?.md"))).toBe(
      "operationUnsupported",
    );
    expect(codeOf(() => assertPortableDestination("notes/plano.md "))).toBe(
      "operationUnsupported",
    );
    expect(codeOf(() => assertPortableDestination("notes/plano:2026.md"))).toBe(
      "operationUnsupported",
    );
    expect(assertPortableDestination("notes/plano-2026.md")).toBe(
      "notes/plano-2026.md",
    );
  });

  it("classifies only the formats Folio handles", () => {
    expect(mediaTypeForPath("notes/paalala.MD")).toBe("text/markdown");
    expect(mediaTypeForPath("notes/paalala.txt")).toBe("text/plain");
    expect(mediaTypeForPath("research/paper.pdf")).toBe("application/pdf");
    expect(mediaTypeForPath("notes/paalala.docx")).toBeUndefined();
  });
});

describe("embedding space isolation", () => {
  const space: EmbeddingSpace = {
    modelId: "intfloat/multilingual-e5-small",
    revision: "r1",
    quantization: "q8",
    dimensions: 384,
    preprocessingFingerprint: "query-passage-v1",
  };

  it("separates revisions and preprocessing even when dimensions match", () => {
    expect(codeOf(() => assertSameEmbeddingSpace(space, { ...space }))).toBe(
      "no-error",
    );
    expect(
      codeOf(() =>
        assertSameEmbeddingSpace(space, { ...space, revision: "r2" }),
      ),
    ).toBe("embeddingSpaceMismatch");
    expect(
      codeOf(() =>
        assertSameEmbeddingSpace(space, {
          ...space,
          preprocessingFingerprint: "raw-text",
        }),
      ),
    ).toBe("embeddingSpaceMismatch");
  });

  it("cannot be confused by a separator inside a model name", () => {
    const left = embeddingSpaceFingerprint({
      ...space,
      modelId: "vendor/model",
      revision: "r1",
    });
    const right = embeddingSpaceFingerprint({
      ...space,
      modelId: "vendor",
      revision: "model/r1",
    });
    expect(left).not.toBe(right);
  });
});
