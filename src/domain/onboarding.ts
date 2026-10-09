import type { DuplicateSet } from "./connections";
import type { DocumentRecord, Relationship } from "./contracts";

export const ONBOARDING_STEPS = [
  "welcome",
  "folder",
  "ai",
  "index",
  "found",
] as const;
export type OnboardingStep = (typeof ONBOARDING_STEPS)[number];

/**
 * Onboarding runs once, in the desktop app, until completed or skipped. The
 * browser preview has no folders to set up, so it never starts there.
 */
export function shouldStartOnboarding(
  nativeAvailable: boolean,
  completed: boolean,
): boolean {
  return nativeAvailable && !completed;
}

export function nextStep(step: OnboardingStep): OnboardingStep {
  const index = ONBOARDING_STEPS.indexOf(step);
  return ONBOARDING_STEPS[Math.min(index + 1, ONBOARDING_STEPS.length - 1)];
}

export function previousStep(step: OnboardingStep): OnboardingStep {
  return ONBOARDING_STEPS[Math.max(ONBOARDING_STEPS.indexOf(step) - 1, 0)];
}

export interface Finding {
  kind: "duplicate" | "link";
  title: string;
  files: Pick<DocumentRecord, "id" | "name" | "relativePath">[];
  /** The linking text, for links. */
  evidence?: string;
}

function folderOf(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash === -1 ? "" : path.slice(0, slash);
}

/**
 * Up to `limit` things Folio actually found in the indexed folder: exact
 * duplicates first, then links between files, preferring links that cross
 * folders. Nothing here is inferred; an empty list means nothing was found.
 */
export function firstFindings(
  relationships: Relationship[],
  duplicates: DuplicateSet[],
  documents: DocumentRecord[],
  limit = 3,
): Finding[] {
  const byId = new Map(documents.map((document) => [document.id, document]));
  const findings: Finding[] = [];
  for (const set of duplicates) {
    const files = set.documents
      .map((item) => byId.get(item.id))
      .filter((file): file is DocumentRecord => Boolean(file));
    if (files.length > 1)
      findings.push({
        kind: "duplicate",
        title: `${files.length} identical files`,
        files,
      });
  }
  const links = relationships
    .filter((edge) => edge.type === "explicitReference")
    .map((edge) => ({
      edge,
      source: byId.get(edge.sourceId),
      target: byId.get(edge.targetId),
    }))
    .filter(
      (
        item,
      ): item is typeof item & {
        source: DocumentRecord;
        target: DocumentRecord;
      } => Boolean(item.source && item.target),
    )
    .sort(
      (a, b) =>
        Number(
          folderOf(b.source.relativePath) !== folderOf(b.target.relativePath),
        ) -
        Number(
          folderOf(a.source.relativePath) !== folderOf(a.target.relativePath),
        ),
    );
  // A link written both ways is one finding.
  const shown = new Set<string>();
  for (const { edge, source, target } of links) {
    const pair = [source.id, target.id].sort().join("\u0000");
    if (shown.has(pair)) continue;
    shown.add(pair);
    findings.push({
      kind: "link",
      title: `${source.name} links to ${target.name}`,
      files: [source, target],
      evidence: edge.evidence[0]?.text,
    });
  }
  return findings.slice(0, limit);
}
