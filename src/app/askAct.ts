import type {
  DocumentId,
  DocumentRecord,
  GroundedResult,
  OperationProposal,
  RetrievalMethod,
  SearchResult,
} from "../domain/contracts";
import type { FolioError } from "../domain/errors";

/** What one Ask & Act request produced. Every kind is read-only. */
export type AskOutcome =
  | { type: "results"; query: string; results: SearchResult[] }
  | { type: "answer"; result: GroundedResult }
  /** The file's summary is running in its Summary tab. */
  | { type: "summary"; document: DocumentRecord }
  | {
      type: "chooseFile";
      purpose: "summarize" | "change";
      candidates: SearchResult[];
    }
  | { type: "clarify"; question: string }
  /** A change Folio understood but can't preview from here yet. */
  | { type: "proposal"; proposal: OperationProposal }
  | { type: "unsupported"; reason: string }
  | { type: "unreadable" }
  /** A change to another file than the one the user chose; never previewed. */
  | { type: "otherFile"; proposal: OperationProposal; chosen: DocumentRecord }
  /**
   * The browser-preview mock only (#66): a fabricated reply, always shown
   * labelled "Practice replies — not a model". Never produced by a real
   * adapter, and never bundled into the desktop app's behavior.
   */
  | { type: "practice"; reply: string; streaming: boolean };

export interface AskTurn {
  id: number;
  request: string;
  action: "find" | "ask";
  status: "running" | "done" | "failed" | "cancelled";
  outcome?: AskOutcome;
  error?: FolioError;
  /** The file the user picked for this request, which a change must target. */
  chosen?: DocumentRecord;
}

/** Earlier turns stay readable; the list is bounded. */
export const MAX_TURNS = 20;

export function addTurn(turns: AskTurn[], turn: AskTurn): AskTurn[] {
  return [...turns, turn].slice(-MAX_TURNS);
}

export function updateTurn(
  turns: AskTurn[],
  id: number,
  change: Partial<AskTurn>,
): AskTurn[] {
  return turns.map((turn) => (turn.id === id ? { ...turn, ...change } : turn));
}

/** The badge for how a result was found. Only embeddings are "semantic". */
export function methodLabel(method: RetrievalMethod): string {
  switch (method) {
    case "keyword":
      return "Keyword match";
    case "semantic":
      return "Semantic match";
    case "hybrid":
      return "Keyword + semantic";
  }
}

const WORD = /[\p{L}\p{N}]+/gu;

/** A short reason a result matches, from the method and the passages. */
export function matchReason(result: SearchResult, query: string): string {
  const text = result.passages
    .map((passage) => passage.text)
    .join(" ")
    .toLocaleLowerCase();
  const terms = [
    ...new Set(
      (query.toLocaleLowerCase().match(WORD) ?? []).filter(
        (term) => term.length > 2,
      ),
    ),
  ].filter((term) => text.includes(term));
  const words = terms.length
    ? `Contains ${terms
        .slice(0, 4)
        .map((term) => `“${term}”`)
        .join(", ")}`
    : "";
  switch (result.method) {
    case "keyword":
      return words || "Matches words in your request";
    case "semantic":
      return "Close in meaning to your request";
    case "hybrid":
      return words
        ? `${words}, and close in meaning`
        : "Close in meaning to your request";
  }
}

/**
 * Results inside one folder of the open folder, or all of them for "".
 * The scope is applied to every result before it is shown.
 */
export function inScope(results: SearchResult[], folder: string) {
  return folder
    ? results.filter((result) =>
        result.document.relativePath.startsWith(`${folder}/`),
      )
    : results;
}

function lowerWords(value: string): string[] {
  return value.normalize("NFKC").toLocaleLowerCase().match(WORD) ?? [];
}

/**
 * Whether `written` spells out `name` as a whole file name, so naming
 * `old-notes.md` doesn't also name `notes.md`.
 */
function writesName(written: string, name: string): boolean {
  const wanted = name.normalize("NFKC").toLocaleLowerCase();
  const escaped = wanted.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(
    `(?<![\\p{L}\\p{N}_.-])${escaped}(?![\\p{L}\\p{N}_-])`,
    "u",
  ).test(written);
}

/**
 * Files the request names. The index scores only file text, so without this
 * "find Sample_Resume.pdf" finds nothing unless the resume's text says so. A
 * file is `named` when every word of its name (extension aside) is in the
 * request, and `partial` when its name shares a longer word with it.
 * `exact` holds the files whose full name, extension included, is written in
 * the request.
 */
export function namedFiles(
  documents: DocumentRecord[],
  request: string,
): {
  named: SearchResult[];
  partial: SearchResult[];
  exact: DocumentRecord[];
} {
  const asked = new Set(lowerWords(request));
  const written = request.normalize("NFKC").toLocaleLowerCase();
  const named: SearchResult[] = [];
  const partial: { result: SearchResult; matched: number }[] = [];
  for (const document of documents) {
    const stem = document.name.replace(/\.[^.]+$/, "");
    const words = [...new Set(lowerWords(stem))];
    if (!words.length) continue;
    const matched = words.filter((word) => asked.has(word));
    const result: SearchResult = {
      document,
      score: matched.length / words.length,
      method: "keyword",
      passages: [],
    };
    if (matched.length === words.length) named.push(result);
    else if (matched.some((word) => word.length > 2))
      partial.push({ result, matched: matched.length });
  }
  const byPath = (a: SearchResult, b: SearchResult) =>
    a.document.relativePath.localeCompare(b.document.relativePath);
  named.sort(byPath);
  return {
    named,
    exact: named
      .map(({ document }) => document)
      .filter((document) => writesName(written, document.name)),
    partial: partial
      .sort((a, b) => b.matched - a.matched || byPath(a.result, b.result))
      .map(({ result }) => result),
  };
}

/**
 * Files a summary request could mean. One clear match, or the one file whose
 * full name the request writes out, is used directly; otherwise the user
 * chooses, and nothing runs until they do.
 */
export function summaryTarget(
  candidates: SearchResult[],
  exact: DocumentRecord[] = [],
): DocumentRecord | null {
  if (exact.length === 1) return exact[0];
  return candidates.length === 1 ? candidates[0].document : null;
}

/** The change a proposal describes, in plain words. */
/**
 * Whether a proposal changes the file the user chose. A create changes no
 * existing file, so it can't contradict the choice.
 */
export function targetsChosenFile(
  proposal: OperationProposal,
  chosenId: DocumentId,
): boolean {
  return proposal.kind === "create" || proposal.documentId === chosenId;
}

export function describeProposal(proposal: OperationProposal): string {
  switch (proposal.kind) {
    case "edit":
      return `Edit ${proposal.relativePath}: replace “${proposal.find}” with “${proposal.replace}”`;
    case "rename":
      return `Rename ${proposal.relativePath} to ${proposal.destinationRelativePath}`;
    case "move":
      return `Move ${proposal.relativePath} to ${proposal.destinationRelativePath}`;
    case "create":
      return `Create ${proposal.destinationRelativePath}`;
  }
}
