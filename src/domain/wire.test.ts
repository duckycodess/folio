import { describe, expect, it } from "vitest";
import cases from "../../fixtures/contracts/contract-cases.json";
import {
  BATCH_STOP_REASONS,
  FOLIO_ERROR_CODES,
  OPERATION_STATUSES,
  SOURCE_OFFSET_UNIT,
  type ActionPlan,
} from "./contracts";
import { isFolioError } from "./errors";
import { hashText } from "./hash";
import {
  documentIdFor,
  embeddingSpaceFingerprint,
  mediaTypeForPath,
  normalizeRelativePath,
  assertPortableDestination,
  workspaceIdForPath,
} from "./identity";
import { canonicalPlanBytes, planDigest } from "./plan";
import { sliceByUtf8Offsets, utf8Length } from "./offsets";

/**
 * These cases come from `fixtures/contracts/generate-contract-cases.py`, a
 * third implementation of the rules in `docs/contracts.md` written from that
 * document rather than from this code. The Rust suite reads the same file, so
 * both languages are pinned to one wire encoding instead of to each other.
 */
describe("cross-language contract fixtures", () => {
  it("agrees on the frozen enumerations", () => {
    expect([...FOLIO_ERROR_CODES]).toEqual(cases.errorCodes);
    expect([...OPERATION_STATUSES]).toEqual(cases.operationStatuses);
    expect([...BATCH_STOP_REASONS]).toEqual(cases.batchStopReasons);
    expect(SOURCE_OFFSET_UNIT).toBe(cases.offsetUnit);
  });

  it("agrees on content hashes", async () => {
    for (const entry of cases.hashes) {
      expect(await hashText(entry.text)).toBe(entry.expected);
    }
  });

  it("agrees on workspace identity for a canonical folder", async () => {
    for (const entry of cases.identity.workspaceId) {
      expect(await workspaceIdForPath(entry.canonicalRootPath)).toBe(
        entry.expected,
      );
    }
  });

  it("agrees on accepted and rejected relative paths", () => {
    for (const entry of cases.identity.normalizeRelativePath.accepted) {
      expect(normalizeRelativePath(entry.input)).toBe(entry.expected);
    }
    for (const entry of cases.identity.normalizeRelativePath.rejected) {
      let code = "no-error";
      try {
        normalizeRelativePath(entry.input);
      } catch (cause) {
        code = isFolioError(cause) ? cause.code : String(cause);
      }
      expect({ input: entry.input, code }).toEqual({
        input: entry.input,
        code: entry.code,
      });
    }
  });

  it("agrees on document identity, including decomposed filenames", () => {
    for (const entry of cases.identity.documentId) {
      expect(documentIdFor(entry.workspaceId, entry.relativePath)).toBe(
        entry.expected,
      );
    }
  });

  it("agrees on destinations no platform can store", () => {
    for (const entry of cases.identity.portableDestination.rejected) {
      let code = "no-error";
      try {
        assertPortableDestination(entry.input);
      } catch (cause) {
        code = isFolioError(cause) ? cause.code : String(cause);
      }
      expect({ input: entry.input, code }).toEqual({
        input: entry.input,
        code: entry.code,
      });
    }
  });

  it("agrees on media types", () => {
    for (const entry of cases.identity.mediaType) {
      expect(mediaTypeForPath(entry.path) ?? null).toBe(entry.expected);
    }
  });

  it("agrees on embedding-space fingerprints", () => {
    for (const entry of cases.identity.embeddingSpaceFingerprint) {
      expect(embeddingSpaceFingerprint(entry.space)).toBe(entry.expected);
    }
  });

  it("agrees on UTF-8 source offsets for Filipino and Taglish text", async () => {
    for (const entry of cases.offsets.cases) {
      expect(utf8Length(entry.text)).toBe(entry.utf8Length);
      expect(entry.text.length).toBe(entry.utf16Length);
      expect(
        sliceByUtf8Offsets(entry.text, entry.passage.start, entry.passage.end),
      ).toBe(entry.passage.text);
      expect(await hashText(entry.text)).toBe(entry.contentHash);
    }
  });

  it("refuses the offsets that fall inside a character", () => {
    for (const entry of cases.offsets.invalid) {
      let code = "no-error";
      try {
        sliceByUtf8Offsets(entry.text, entry.start, entry.end);
      } catch (cause) {
        code = isFolioError(cause) ? cause.code : String(cause);
      }
      expect(code).toBe("internal");
    }
  });

  it("agrees on canonical plan bytes and digests", async () => {
    for (const entry of cases.plans) {
      const plan = entry.plan as unknown as ActionPlan;
      const canonical = canonicalPlanBytes(plan);
      expect(new TextDecoder().decode(canonical)).toBe(entry.canonical);
      expect(canonical.length).toBe(entry.canonicalByteLength);
      expect(await planDigest(plan)).toBe(entry.digest);
      expect(plan.digest).toBe(entry.digest);
    }
  });
});
