import type {
  PreparingProgress,
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

/**
 * Files a summary request could mean. One clear match is used directly;
 * otherwise the user chooses, and nothing runs until they do.
 */
export function summaryTarget(
  candidates: SearchResult[],
): DocumentRecord | null {
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

/**
 * What a request is doing while it prepares the folder, or `undefined` when
 * there is nothing to say (no work, or a total of zero).
 */
export function preparingLabel(
  progress: PreparingProgress | null | undefined,
): string | undefined {
  if (!progress || progress.total <= 0) return undefined;
  const done = Math.min(progress.processed, progress.total);
  return progress.phase === "reading"
    ? `Reading your files: ${done} of ${progress.total}`
    : `Preparing search by meaning: ${done} of ${progress.total} passages`;
}
