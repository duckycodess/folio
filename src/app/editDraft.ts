import type { ContentHash, DocumentId } from "../domain/contracts";

/** The revision the user is editing: the exact text and hash Folio read. */
export interface EditBase {
  documentId: DocumentId;
  content: string;
  contentHash: ContentHash;
}

export interface EditDraft {
  base: EditBase;
  /** What the textarea shows. */
  draft: string;
  /** True when the file changed under a draft the user had started. */
  changedUnder: boolean;
}

/** What a textarea shows for `text`: every line break as `\n`. */
export function asTyped(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

/**
 * Edit `fresh`, keeping a draft the user already started on this file.
 * Returns `null` when nothing changes: the same revision was read again.
 * A draft that already matches the new text (for example, the user's own
 * saved edit) isn't a conflict, so it starts afresh without a warning.
 */
export function adoptRevision(
  current: { base: EditBase | null; draft: string },
  fresh: EditBase,
): EditDraft | null {
  const { base, draft } = current;
  if (base?.documentId === fresh.documentId) {
    if (base.contentHash === fresh.contentHash) return null;
    if (draft !== asTyped(base.content) && draft !== asTyped(fresh.content))
      return { base: fresh, draft, changedUnder: true };
  }
  return { base: fresh, draft: asTyped(fresh.content), changedUnder: false };
}
