import { describe, it } from "vitest";

/**
 * Cases that cannot be proved yet. They are listed as pending on purpose: a
 * mock that returns a successful write would make the suite green without any
 * file ever changing, which is exactly the claim this repository must not make.
 *
 * The native writer is https://github.com/duckycodess/folio/issues/5. The local
 * model adapters are issues #4 and #8.
 */
describe("pending: native write engine (issue #5)", () => {
  it.todo(
    "applies an approved edit to a real temporary file and refreshes the index",
  );
  it.todo(
    "leaves the original file untouched when the write fails midway and reports no success",
  );
  it.todo(
    "keeps earlier successful operations durable when a later one fails on disk",
  );
  it.todo(
    "stops before the next operation after cancellation and keeps the finished change",
  );
  it.todo(
    "restores a real batch through Undo only when every file still matches what Folio saved",
  );
  it.todo("refuses a rename when the destination exists on disk");
  it.todo("records recoverable history that survives an application restart");
});

describe("pending: local providers (issues #4 and #8)", () => {
  it.todo(
    "retrieves a Filipino document for an English query through multilingual embeddings",
  );
  it.todo("summarizes a Filipino document and cites its source passages");
  it.todo("reports insufficient evidence instead of inventing an answer");
  it.todo("cancels a running generation and reports the cancelled code");
  it.todo("refuses a second concurrent generative request as providerBusy");
  it.todo("rebuilds or separates the index when the embedding space changes");
});

describe("pending: platform evidence (issues #2 and #10)", () => {
  it.todo("starts the desktop shell on actual Windows and macOS");
  it.todo("cancels the native folder picker without authorizing a folder");
  it.todo("reports a permission error from the native folder picker");
});
