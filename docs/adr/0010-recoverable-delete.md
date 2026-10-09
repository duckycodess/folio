# Keep a deleted file's bytes in history and restore it only where its name is free

The Graph's node actions need a way to remove a TXT or Markdown file, and ADR 0001 requires that a physical change be previewed exactly and approved. Deleting one file from the Graph is a manual action: the user picks the file, sees the exact preview and the deletion impacts, and approves it. Deletion from a natural-language request stays out of scope.

Folio keeps the deleted file's exact bytes in its own history rather than moving the file to the system trash. The trash behaves differently on Windows and macOS, and Folio could not verify what it holds or restore from it reliably. The history entry is stored before the file is removed, so if it can't be stored nothing is deleted. A file that changed after the preview is kept.

Undo re-creates the file at its previous path with an exclusive create, and only when nothing uses that name; it never replaces another file. The restored file gets the same document identity back, but not its earlier permissions or modification time. Deleted content follows the same retention as edits: it is kept for the 100 most recent applied plans, after which the deletion stays listed but can no longer be undone. PDFs are read-only, so they are never deleted.

Before approval, the deletion impacts list the files whose links to it will break, byte-identical copies that remain, and relations the local model found. They are for review only: Folio never changes those files, and they never become operations.

Decided with Gab on 2026-10-10 for issue #44.
