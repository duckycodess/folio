import { describe, it } from "vitest";

/**
 * Cases that cannot be proved yet. They are listed as pending on purpose: a
 * mock that returns a successful write would make the suite green without any
 * file ever changing, which is exactly the claim this repository must not make.
 *
 * The local model adapters are issues #4 and #8.
 */
// The native write engine (issue #5) is no longer pending: applying an approved
// edit, keeping earlier successes when a later write fails, cancellation,
// refusing an existing destination, whole-batch Undo and history that survives
// a restart are exercised against real temporary folders in
// `src-tauri/src/writer.rs`.

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
