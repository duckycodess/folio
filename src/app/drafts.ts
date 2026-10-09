import { useState } from "react";

/**
 * Text the user is still writing. Held above the views, so it survives errors,
 * a trip to Model Lab and switching views. Rename names are kept per file, so a
 * name typed for one file never appears for another.
 */
export interface Drafts {
  instruction: string;
  setInstruction: (text: string) => void;
  renameName: (documentId: string) => string;
  setRenameName: (documentId: string, name: string) => void;
  /** Call once a rename succeeds, so an old draft can't reappear after Undo. */
  clearRenameName: (documentId: string) => void;
}

export function useDrafts(): Drafts {
  const [instruction, setInstruction] = useState("");
  const [renames, setRenames] = useState<Record<string, string>>({});
  return {
    instruction,
    setInstruction,
    renameName: (documentId) => renames[documentId] ?? "",
    setRenameName: (documentId, name) =>
      setRenames((all) => ({ ...all, [documentId]: name })),
    clearRenameName: (documentId) =>
      setRenames((all) => {
        const rest = { ...all };
        delete rest[documentId];
        return rest;
      }),
  };
}
