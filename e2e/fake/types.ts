import type {
  ContentHash,
  FolioErrorCode,
  FolioErrorDetails,
  MediaType,
  ModelDescriptor,
  RelativePath,
  ScanSummary,
} from "../../src/domain/contracts";

/** One page of an extracted PDF, as UTF-16 indexes into the extracted text. */
export interface FakePageRange {
  page: number;
  startIndex: number;
  endIndex: number;
}

/**
 * One file in the fake workspace. `content` is the text Folio would extract;
 * for a PDF the file's own bytes are described separately, because its hash and
 * size are the file's, never the extracted text's.
 */
export interface FakeFileInput {
  relativePath: RelativePath;
  content: string;
  mediaType: MediaType;
  modifiedAtMs: number | null;
  /** PDFs only: the exact file size and hash, and the extracted pages. */
  fileSizeBytes?: number;
  fileContentHash?: ContentHash;
  pages?: FakePageRange[];
}

/** A file the fake lists under `skipped` instead of presenting as a document. */
export interface FakeSkippedEntry {
  displayName: string;
  code: FolioErrorCode;
}

export interface FakeNativeOptions {
  /** Contains no `:`; document identities are `${workspaceId}:${relativePath}`. */
  workspaceId: string;
  rootPath: string;
  authorizedAt: number;
  files: FakeFileInput[];
  /** True when the persistent index already holds the folder. */
  preIndexed: boolean;
  /** Milliseconds a scan spends per file, so Stop has something to stop. */
  scanStepMs: number;
  /** How long a prepared plan stays valid. */
  planLifetimeMs: number;
  skipped: FakeSkippedEntry[];
  /** `choose_workspace` resolves to `null`, as a dismissed picker does. */
  dismissFolderPicker: boolean;
  /**
   * Groups `suggest_collections` returns, standing in for the real core's
   * embedding and generation models. Without them the fake reports that no
   * embedding model is set up. A test double: it exercises the UI's handling
   * of suggestions, never grouping or naming quality.
   */
  collectionGroups?: FakeCollectionGroup[];
  /**
   * What `suggest_file_changes` returns in place of the real core's models: a
   * name a model would write for a file, and a folder its files would be
   * closer to. Without `destinations`, the fake reports no embedding model.
   */
  modelFilenames?: { path: RelativePath; name: string }[];
  destinations?: { path: RelativePath; folder: RelativePath }[];
  /**
   * A rename `interpret_request` proposes in place of the real core's model,
   * whatever the request says. Without it, the fake reports no model.
   */
  interpretRename?: { path: RelativePath; destination: RelativePath };
  /** Pinned manifest metadata only; the fake never installs or runs a model. */
  models?: ModelDescriptor[];
  runtime?: { id: string; version: string; bytes: number };
}

/** One canned suggested collection: its files, and the name a model would write. */
export interface FakeCollectionGroup {
  paths: RelativePath[];
  name?: string;
}

/** A failure to inject into the next (or every) call of one command. */
export interface FakeFailure {
  code: FolioErrorCode;
  message?: string;
  details?: FolioErrorDetails;
}

/**
 * What the native writer should do to the next approved plan. Every field
 * describes something the real writer can report; nothing here invents an
 * outcome the frozen contract does not allow.
 */
export interface FakeWriterBehaviour {
  /** This operation fails before touching its file; the batch stops there. */
  failAtIndex?: number;
  failCode?: FolioErrorCode;
  /**
   * This operation changes its file and then reports `historyRequired`: the
   * change is real, but no reversal was recorded for it.
   */
  historyRequiredAtIndex?: number;
  /** False when bookkeeping after the writes failed. The writes still happened. */
  historySettled?: boolean;
  /** False when the local index has not caught up with the changes. */
  indexRefreshed?: boolean;
  /** An Undo that stops after this many entries, leaving the rest pending. */
  undoStopAfter?: number;
}

/** The handle the browser journeys drive the fake with, as `window.__folioFake`. */
export interface FakeControl {
  readonly sentinel: string;
  /** The next call of `command` rejects with this payload. */
  failNext(command: string, failure: FakeFailure): void;
  /** Every call of `command` rejects until `clearFailures`. */
  failAlways(command: string, failure: FakeFailure): void;
  clearFailures(): void;
  setScanStepMs(ms: number): void;
  setSkipped(entries: FakeSkippedEntry[]): void;
  setWriter(behaviour: FakeWriterBehaviour): void;
  /** Another application edits a file behind Folio's back. */
  externalEdit(relativePath: RelativePath, content: string): void;
  /** Another application deletes a file behind Folio's back. */
  externalDelete(relativePath: RelativePath): void;
  /** Every prepared plan's validity window ends in the past. */
  expirePlans(): void;
  /** The current text of a file, or `null` when it is not there. */
  readFile(relativePath: RelativePath): string | null;
  /** Every path currently in the fake workspace, sorted. */
  listFiles(): RelativePath[];
  /** Every command the UI has invoked, in order. */
  calls(): string[];
  /**
   * The summary the last finished scan resolved with. Stopping a scan is not a
   * failure, so a journey can check the reply itself rather than only the
   * screen the UI drew from it.
   */
  lastScan(): ScanSummary | null;
}
