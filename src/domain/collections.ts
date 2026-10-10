import type {
  CollectionSuggestions,
  KeptMember,
  SuggestedCollection,
  VirtualCollection,
} from "./contracts";

/** The native core's limit; the name is checked there again. */
export const MAX_COLLECTION_NAME_CHARS = 80;

/** Whitespace collapsed, as the native core stores it. */
export function cleanCollectionName(raw: string): string {
  return raw.split(/\s+/u).filter(Boolean).join(" ");
}

/** Why a name can't be kept, or `null` when it can. */
export function nameProblem(raw: string): string | null {
  const name = cleanCollectionName(raw);
  if (!name) return "Give the collection a name.";
  if ([...name].length > MAX_COLLECTION_NAME_CHARS)
    return `Use at most ${MAX_COLLECTION_NAME_CHARS} characters.`;
  // Control characters (C0, DEL and C1) are refused natively too.
  if (/[\u0000-\u001f\u007f-\u009f]/u.test(name))
    return "Remove the control characters from the name.";
  // So are invisible formatting characters, such as bidirectional overrides,
  // which would make the name display differently from its text.
  if (
    /[\u00ad\u061c\u180e\u200b-\u200f\u202a-\u202e\u2060-\u2069\ufeff]/u.test(
      name,
    )
  )
    return "Remove the invisible formatting characters from the name.";
  return null;
}

/** What the user is about to keep from one suggested group. */
export interface CollectionDraft {
  name: string;
  /** Members still ticked, by document. */
  chosen: string[];
}

export function draftFor(group: SuggestedCollection): CollectionDraft {
  return {
    name: group.name?.text ?? "",
    chosen: group.members.map((member) => member.documentId),
  };
}

/**
 * Where the name on screen came from. A generated name stays labelled as
 * generated only while it is exactly what the model wrote.
 */
export function nameOrigin(
  group: SuggestedCollection,
  draft: CollectionDraft,
): "generated" | "edited" | "typed" | "empty" {
  const name = cleanCollectionName(draft.name);
  if (!name) return "empty";
  if (!group.name) return "typed";
  return name === group.name.text ? "generated" : "edited";
}

/** The members to keep, each with the revision the analysis read. */
export function keptMembers(
  group: SuggestedCollection,
  draft: CollectionDraft,
): KeptMember[] {
  return group.members
    .filter((member) => draft.chosen.includes(member.documentId))
    .map((member) => ({
      documentId: member.documentId,
      expectedContentHash: member.contentHash,
    }));
}

/** Why this draft can't be kept yet, or `null`. */
export function keepProblem(
  group: SuggestedCollection,
  draft: CollectionDraft,
): string | null {
  if (keptMembers(group, draft).length < 2)
    return "Keep at least two files in the collection.";
  return nameProblem(draft.name);
}

/** One line about how the analysis went, or `null` when there is nothing to say. */
export function suggestionsNotice(
  result: CollectionSuggestions,
): { tone: "info" | "warning"; text: string } | null {
  if (result.status === "embeddingModelMissing")
    return {
      tone: "info",
      text: "Grouping files by meaning needs a local embedding model. Set one up in Model Lab.",
    };
  const notes: string[] = [];
  if (result.truncated)
    notes.push(
      `Folio compared the first ${result.analyzedDocumentCount} text files by path; the rest weren't analyzed.`,
    );
  switch (result.naming) {
    case "generationModelMissing":
      notes.push(
        "Naming groups needs a local generation model, so these groups have no name yet. Type one to keep a group.",
      );
      break;
    case "cancelled":
      notes.push(
        "Naming stopped. Groups without a name can still be kept once you type one.",
      );
      break;
    case "failed":
      notes.push(
        `Folio couldn't name these groups${result.namingError ? `: ${result.namingError.message}` : "."} Type a name to keep a group.`,
      );
      break;
  }
  if (!notes.length) return null;
  return {
    tone: result.naming === "failed" ? "warning" : "info",
    text: notes.join(" "),
  };
}

/** Files Folio can still find, then missing ones, each by path. */
export function memberCounts(collection: VirtualCollection): {
  present: number;
  missing: number;
} {
  const missing = collection.members.filter((member) => member.missing).length;
  return { present: collection.members.length - missing, missing };
}

/** "3 files" / "3 files, 1 missing". */
export function membersLabel(collection: VirtualCollection): string {
  const { present, missing } = memberCounts(collection);
  const files = `${present} ${present === 1 ? "file" : "files"}`;
  return missing ? `${files}, ${missing} missing` : files;
}
