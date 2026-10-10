import type {
  DocumentId,
  DocumentRecord,
  GroundedResult,
  SourcePassage,
} from "../domain/contracts";
import type { FolioError } from "../domain/errors";
import { generatedBy } from "./generatedBy";
import { utf8Length } from "../domain/offsets";

/** One file's summary, wherever it was asked for (Summary tab or Ask & Act). */
export type SummaryEntry =
  | { status: "running"; startedAt: number }
  | { status: "done"; result: GroundedResult; madeAt: number }
  | { status: "failed"; error: FolioError }
  | { status: "cancelled" };

/** Summaries kept for this session, most recent last. Bounded. */
export const MAX_SUMMARIES = 20;

export interface SummaryStore {
  get: (documentId: DocumentId) => SummaryEntry | undefined;
  /** The file whose summary is being generated; only one runs at a time. */
  running: () => DocumentId | null;
  set: (documentId: DocumentId, entry: SummaryEntry | undefined) => void;
  subscribe: (listener: () => void) => () => void;
  /** Changes whenever anything does, for `useSyncExternalStore`. */
  version: () => number;
}

export function createSummaryStore(limit = MAX_SUMMARIES): SummaryStore {
  const entries = new Map<DocumentId, SummaryEntry>();
  const listeners = new Set<() => void>();
  let version = 0;
  return {
    get: (documentId) => entries.get(documentId),
    running: () => {
      for (const [documentId, entry] of entries)
        if (entry.status === "running") return documentId;
      return null;
    },
    set: (documentId, entry) => {
      // Clearing never drops a running summary: it would release the
      // one-at-a-time lock while the model is still working.
      if (!entry && entries.get(documentId)?.status === "running") return;
      entries.delete(documentId);
      if (entry) entries.set(documentId, entry);
      // Drop the oldest finished summaries; never a running one.
      for (const [id, kept] of entries) {
        if (entries.size <= limit) break;
        if (kept.status !== "running") entries.delete(id);
      }
      version += 1;
      listeners.forEach((listener) => listener());
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    version: () => version,
  };
}

/**
 * The file changed after the summary was made: some cited passage was taken
 * from a different revision than the one Folio has now.
 */
export function isStale(
  result: GroundedResult,
  document: Pick<DocumentRecord, "id" | "contentHash">,
): boolean {
  if (!document.contentHash) return false;
  return result.sentences.some((sentence) =>
    sentence.citations.some(
      (citation) =>
        citation.documentId === document.id &&
        citation.documentContentHash !== document.contentHash,
    ),
  );
}

/**
 * How much of the file's text the summary read, as a whole percentage.
 * Coverage ranges are UTF-8 offsets into the extracted text, so they are
 * compared with that text, never with the file's size on disk (a PDF is
 * mostly fonts and structure). Unknown until the text has been read.
 */
export function coveredPercent(
  result: GroundedResult,
  document: Pick<DocumentRecord, "id" | "content">,
): number | null {
  const entry = result.coverageRanges.find(
    (coverage) => coverage.documentId === document.id,
  );
  if (!entry) return null;
  if (entry.complete) return 100;
  const textBytes =
    document.content === undefined ? 0 : utf8Length(document.content);
  if (textBytes <= 0) return null;
  const bytes = entry.ranges.reduce(
    (total, range) => total + Math.max(0, range.end - range.start),
    0,
  );
  return Math.min(99, Math.floor((bytes / textBytes) * 100));
}

/** `notes/plan.md` → `notes/plan summary.md`, avoiding names already taken. */
export function summaryPath(
  relativePath: string,
  taken: Iterable<string>,
): string {
  const used = new Set([...taken].map((path) => path.toLocaleLowerCase()));
  const slash = relativePath.lastIndexOf("/");
  const folder = slash >= 0 ? relativePath.slice(0, slash + 1) : "";
  const name = relativePath.slice(slash + 1);
  const stem = name.includes(".") ? name.slice(0, name.lastIndexOf(".")) : name;
  for (let attempt = 1; ; attempt++) {
    const candidate = `${folder}${stem} summary${attempt > 1 ? ` ${attempt}` : ""}.md`;
    if (!used.has(candidate.toLocaleLowerCase())) return candidate;
  }
}

/** The local calendar date, as YYYY-MM-DD. */
function localDate(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

function quote(passage: SourcePassage): string {
  return passage.text.replace(/\s+/g, " ").trim();
}

/**
 * The Markdown a saved summary contains: what it summarizes, that it was
 * generated and by which model, each point, and the passages it cites.
 */
export function summaryMarkdown(
  document: Pick<DocumentRecord, "name" | "relativePath">,
  result: GroundedResult,
  madeAt: Date,
): string {
  const sources: SourcePassage[] = [];
  const index = (passage: SourcePassage) => {
    const key = `${passage.documentId}|${passage.start}|${passage.end}`;
    const at = sources.findIndex(
      (known) => `${known.documentId}|${known.start}|${known.end}` === key,
    );
    if (at >= 0) return at + 1;
    sources.push(passage);
    return sources.length;
  };
  const points = result.sentences.map((sentence) => {
    const marks = sentence.citations.map((citation) => `[${index(citation)}]`);
    return `- ${sentence.text.trim()}${marks.length ? ` ${marks.join("")}` : ""}`;
  });
  const kind = result.kind === "partialSummary" ? "Partial summary" : "Summary";
  return [
    `# ${kind} of ${document.name}`,
    "",
    `${generatedBy(result)} on ${localDate(madeAt)} from \`${document.relativePath}\`. Not reviewed for accuracy; check the sources.`,
    "",
    ...points,
    "",
    "## Sources",
    "",
    ...sources.map(
      (passage, at) =>
        `${at + 1}. ${document.relativePath}${passage.page !== undefined ? `, page ${passage.page}` : ""}: “${quote(passage)}”`,
    ),
    "",
  ].join("\n");
}
