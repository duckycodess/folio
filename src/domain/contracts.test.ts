import { describe, expect, it } from "vitest";
import cases from "../../fixtures/contracts/contract-cases.json";
import providerError from "../../fixtures/contracts/provider-error.json";
import modelDescriptor from "../../fixtures/contracts/model-descriptor.json";
import groundedAnswer from "../../fixtures/contracts/grounded-answer.json";
import interpretationResult from "../../fixtures/contracts/interpretation-result.json";
import benchmarkRecord from "../../fixtures/contracts/benchmark-record.json";
import submissionChecklist from "../../fixtures/documents/projects/submission-checklist.md?raw";
import { assertPassageMatches, sliceByUtf8Offsets } from "./offsets";
import type {
  BenchmarkRecord,
  BenchmarkResult,
  GroundedAnswer,
  GroundedResult,
  ModelDescriptor,
  FolioErrorPayload,
  SourcePassage,
} from "./contracts";

function hasOnlyCamelCaseKeys(value: unknown): boolean {
  if (Array.isArray(value)) return value.every(hasOnlyCamelCaseKeys);
  if (!value || typeof value !== "object") return true;
  return Object.entries(value).every(([key, child]) => {
    expect(key).not.toMatch(/_/);
    return hasOnlyCamelCaseKeys(child);
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isFolioErrorPayload(value: unknown): value is FolioErrorPayload {
  if (!isRecord(value)) return false;
  return (
    typeof value.code === "string" &&
    typeof value.message === "string" &&
    (value.details === undefined ||
      (isRecord(value.details) &&
        Object.values(value.details).every(
          (detail) => typeof detail === "string",
        )))
  );
}

function isModelDescriptor(value: unknown): value is ModelDescriptor {
  if (!isRecord(value)) return false;
  return (
    typeof value.id === "string" &&
    ["embedding", "generation"].some((role) => role === value.role) &&
    typeof value.repo === "string" &&
    typeof value.revision === "string" &&
    Array.isArray(value.files) &&
    value.files.every(
      (file) =>
        isRecord(file) &&
        typeof file.path === "string" &&
        typeof file.sha256 === "string" &&
        typeof file.bytes === "number",
    ) &&
    typeof value.quantization === "string" &&
    typeof value.license === "string" &&
    typeof value.runtime === "string" &&
    typeof value.optionalPack === "boolean"
  );
}

function requireFolioErrorPayload(value: unknown): FolioErrorPayload {
  if (!isFolioErrorPayload(value)) {
    throw new Error("provider-error.json does not match FolioErrorPayload");
  }
  return value;
}

function requireModelDescriptor(value: unknown): ModelDescriptor {
  if (!isModelDescriptor(value)) {
    throw new Error("model-descriptor.json does not match ModelDescriptor");
  }
  return value;
}

function isSourcePassage(value: unknown): value is SourcePassage {
  return (
    isRecord(value) &&
    typeof value.documentId === "string" &&
    typeof value.documentContentHash === "string" &&
    value.offsetUnit === "utf8Byte" &&
    typeof value.start === "number" &&
    typeof value.end === "number" &&
    typeof value.text === "string"
  );
}

function isGroundedResult(value: unknown): value is GroundedResult {
  if (!isRecord(value)) return false;
  const isCoverageRange = (range: unknown): boolean =>
    isRecord(range) &&
    typeof range.start === "number" &&
    typeof range.end === "number";
  const isCoverageEntry = (entry: unknown): boolean =>
    isRecord(entry) &&
    typeof entry.documentId === "string" &&
    typeof entry.documentContentHash === "string" &&
    entry.offsetUnit === "utf8Byte" &&
    Array.isArray(entry.ranges) &&
    entry.ranges.every(isCoverageRange) &&
    typeof entry.complete === "boolean";
  const isSentence = (sentence: unknown): boolean =>
    isRecord(sentence) &&
    typeof sentence.text === "string" &&
    Array.isArray(sentence.citations) &&
    sentence.citations.every(isSourcePassage);

  return (
    typeof value.text === "string" &&
    Array.isArray(value.sources) &&
    value.sources.every(isSourcePassage) &&
    Array.isArray(value.coverage) &&
    value.coverage.every((documentId) => typeof documentId === "string") &&
    typeof value.modelId === "string" &&
    typeof value.revision === "string" &&
    ["fileSummary", "partialSummary", "answer", "insufficientEvidence"].some(
      (kind) => kind === value.kind,
    ) &&
    Array.isArray(value.sentences) &&
    value.sentences.every(isSentence) &&
    Array.isArray(value.coverageRanges) &&
    value.coverageRanges.every(isCoverageEntry) &&
    typeof value.uncitedSentenceCount === "number"
  );
}

function requireGroundedResult(value: unknown): GroundedResult {
  if (!isGroundedResult(value)) {
    throw new Error("grounded-answer.json does not match GroundedResult");
  }
  return value;
}

describe("native contract goldens", () => {
  it("keeps provider and model keys typed and camelCase", () => {
    const error = requireFolioErrorPayload(providerError);
    const descriptor = requireModelDescriptor(modelDescriptor);
    expect(error.code).toBe("modelNotInstalled");
    expect(error.details?.modelId).toBe("generation");
    expect(descriptor.files[0].sha256).toHaveLength(64);
    expect(hasOnlyCamelCaseKeys(error)).toBe(true);
    expect(hasOnlyCamelCaseKeys(descriptor)).toBe(true);
  });

  it("keeps grounded citations and interpretation statuses typed", () => {
    const result = requireGroundedResult(groundedAnswer);
    const answer: GroundedAnswer = result;
    expect(result.kind).toBe("fileSummary");
    expect(answer.sources[0].documentId).toBe(
      "fixtures:projects/submission-checklist.md",
    );
    expect(answer.coverage).toEqual([
      "fixtures:projects/submission-checklist.md",
    ]);
    expect(answer.revision).toBe("revision-a");
    expect(result.coverageRanges[0].offsetUnit).toBe("utf8Byte");
    expect(result.sources[0].start).toBe(43);
    expect(result.sources[0].end).toBe(88);
    expect(interpretationResult.status).toBe("needsClarification");
    expect(hasOnlyCamelCaseKeys(answer)).toBe(true);
    expect(hasOnlyCamelCaseKeys(result)).toBe(true);
    expect(hasOnlyCamelCaseKeys(interpretationResult)).toBe(true);

    const expectedHash =
      "sha256:8b3538ff1e91ed23104eb5ca6083bf4e44ea37cc8bcb23878d384ca1346bc15a";
    for (const passage of [
      ...result.sources,
      ...result.sentences.flatMap((sentence) => sentence.citations),
    ]) {
      expect(passage.documentContentHash).toBe(expectedHash);
      assertPassageMatches(passage, submissionChecklist, expectedHash);
      expect(
        sliceByUtf8Offsets(submissionChecklist, passage.start, passage.end),
      ).toBe(passage.text);
    }
    for (const entry of result.coverageRanges) {
      expect(entry.documentContentHash).toBe(expectedHash);
      for (const range of entry.ranges) {
        expect(
          sliceByUtf8Offsets(submissionChecklist, range.start, range.end),
        ).toBe(submissionChecklist.slice(0));
      }
    }
  });

  it("checks the shared UTF-8 offset cases, including non-ASCII text", () => {
    expect(
      cases.offsets.cases.some((entry) => /[^\x00-\x7f]/u.test(entry.text)),
    ).toBe(true);
    for (const entry of cases.offsets.cases) {
      const passage: SourcePassage = {
        documentId: `contract:${entry.label}`,
        documentContentHash: entry.contentHash,
        offsetUnit: "utf8Byte",
        start: entry.passage.start,
        end: entry.passage.end,
        text: entry.passage.text,
      };
      assertPassageMatches(passage, entry.text, entry.contentHash);
      expect(sliceByUtf8Offsets(entry.text, passage.start, passage.end)).toBe(
        passage.text,
      );
    }
  });
});

function isBenchmarkRecord(value: unknown): value is BenchmarkRecord {
  if (!isRecord(value)) return false;
  const text = (key: string): boolean => typeof value[key] === "string";
  const num = (key: string): boolean => typeof value[key] === "number";
  const nullableNum = (key: string): boolean =>
    value[key] === null || typeof value[key] === "number";
  const timing = value.timing;
  const settings = value.serverSettings;
  const conditions = value.conditions;
  return (
    ["retrieval", "interpretation", "summary", "edit"].some(
      (task) => task === value.task,
    ) &&
    ["caseId", "modelId", "revision", "quantization", "runtime", "hardware"]
      .concat(["id", "runId", "promptSha256", "outputSha256"])
      .every(text) &&
    ["contextTokens", "taskDurationMs", "modelDiskBytes", "modelFileBytes"]
      .concat(["createdAt"])
      .every(num) &&
    typeof value.cold === "boolean" &&
    (value.correctness === null || typeof value.correctness === "boolean") &&
    nullableNum("peakProcessRamBytes") &&
    value.schemaVersion === 1 &&
    isRecord(timing) &&
    ["firstRequestAfterServerRestart", "immediateRepeat"].some(
      (position) => position === timing.requestPosition,
    ) &&
    isRecord(settings) &&
    typeof settings.cachePrompt === "boolean" &&
    isRecord(conditions) &&
    conditions.pageCache === "notControlled" &&
    Array.isArray(value.memory) &&
    value.memory.every(
      (entry) =>
        isRecord(entry) &&
        typeof entry.scope === "string" &&
        typeof entry.method === "string" &&
        (entry.peakBytes === null || typeof entry.peakBytes === "number") &&
        (entry.peakBytes !== null ||
          typeof entry.unavailableReason === "string"),
    ) &&
    Array.isArray(value.objectiveChecks) &&
    Array.isArray(value.reviews) &&
    isRecord(value.apply) &&
    value.apply.status === "notRun"
  );
}

describe("Model Lab record contract (issue #8)", () => {
  it("matches the golden record and stays assignable to BenchmarkResult", () => {
    expect(isBenchmarkRecord(benchmarkRecord)).toBe(true);
    const record = benchmarkRecord as unknown as BenchmarkRecord;
    const frozen: BenchmarkResult = record;
    expect(frozen.caseId).toBe("summary-fil");
    expect(hasOnlyCamelCaseKeys(benchmarkRecord)).toBe(true);
  });

  it("maps the frozen fields from the richer record", () => {
    const record = benchmarkRecord as unknown as BenchmarkRecord;
    expect(record.modelDiskBytes).toBe(record.modelFileBytes);
    expect(record.contextTokens).toBe(record.conditions.nCtx);
    expect(record.runtime).toContain(record.runtimeDetail.version);
    expect(record.cold).toBe(
      record.timing.requestPosition === "firstRequestAfterServerRestart",
    );
  });

  it("never grades a summary and carries no aggregate score", () => {
    const record = benchmarkRecord as unknown as BenchmarkRecord;
    expect(record.task).toBe("summary");
    expect(record.correctness).toBeNull();
    expect(record.reviews).toEqual([]);
    const keys: string[] = [];
    const collect = (value: unknown): void => {
      if (Array.isArray(value)) value.forEach(collect);
      else if (isRecord(value)) {
        for (const [key, child] of Object.entries(value)) {
          keys.push(key);
          collect(child);
        }
      }
    };
    collect(benchmarkRecord);
    expect(
      keys.filter((key) => /score|aggregate|overall|rank/i.test(key)),
    ).toEqual([]);
  });

  it("requires a reason whenever a peak is unavailable", () => {
    const record = benchmarkRecord as unknown as BenchmarkRecord;
    for (const entry of record.memory) {
      if (entry.peakBytes === null) {
        expect(entry.unavailableReason).toBeTruthy();
      }
    }
    const withoutReason = {
      ...benchmarkRecord,
      memory: [{ ...benchmarkRecord.memory[0], unavailableReason: undefined }],
    };
    expect(isBenchmarkRecord(withoutReason)).toBe(false);
  });

  it("rejects a record that claims to be controlled or applied", () => {
    expect(
      isBenchmarkRecord({
        ...benchmarkRecord,
        conditions: { ...benchmarkRecord.conditions, pageCache: "cold" },
      }),
    ).toBe(false);
    expect(
      isBenchmarkRecord({ ...benchmarkRecord, apply: { status: "applied" } }),
    ).toBe(false);
  });
});
