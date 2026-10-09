import { describe, expect, it } from "vitest";
import type { DocumentRecord, Relationship } from "./contracts";
import {
  firstFindings,
  nextStep,
  previousStep,
  shouldStartOnboarding,
} from "./onboarding";

const doc = (relativePath: string) =>
  ({
    id: relativePath,
    relativePath,
    name: relativePath.split("/").at(-1),
  }) as DocumentRecord;
const link = (sourceId: string, targetId: string, text = "see plan") =>
  ({
    type: "explicitReference",
    sourceId,
    targetId,
    evidence: [{ text }],
  }) as unknown as Relationship;

describe("onboarding", () => {
  it("starts once in the desktop app and never in the browser preview", () => {
    expect(shouldStartOnboarding(true, false)).toBe(true);
    expect(shouldStartOnboarding(true, true)).toBe(false);
    expect(shouldStartOnboarding(false, false)).toBe(false);
  });

  it("moves through the steps without running off either end", () => {
    expect(nextStep("welcome")).toBe("folder");
    expect(nextStep("found")).toBe("found");
    expect(previousStep("welcome")).toBe("welcome");
    expect(previousStep("index")).toBe("ai");
  });
});

describe("first findings", () => {
  const files = [
    doc("projects/project-plan.md"),
    doc("meetings/meeting-notes.md"),
    doc("projects/checklist.md"),
    doc("archive/project-plan-copy.md"),
  ];

  it("shows exact duplicates first, then links that cross folders", () => {
    const found = firstFindings(
      [
        link("projects/checklist.md", "projects/project-plan.md"),
        link(
          "meetings/meeting-notes.md",
          "projects/project-plan.md",
          "[plan](../projects/project-plan.md)",
        ),
      ],
      [
        {
          documents: [
            { id: "projects/project-plan.md" },
            { id: "archive/project-plan-copy.md" },
          ],
        },
      ],
      files,
    );
    expect(found.map((item) => item.title)).toEqual([
      "2 identical files",
      "meeting-notes.md links to project-plan.md",
      "checklist.md links to project-plan.md",
    ]);
    expect(found[1].evidence).toContain("../projects/project-plan.md");
  });

  it("ignores files the folder no longer lists and invents nothing", () => {
    expect(
      firstFindings([link("gone.md", "projects/project-plan.md")], [], files),
    ).toEqual([]);
    expect(
      firstFindings(
        [],
        [{ documents: [{ id: "projects/checklist.md" }] }],
        files,
      ),
    ).toEqual([]);
  });

  it("stops at the limit", () => {
    const many = files.slice(1).map((file) => link(file.id, files[0].id));
    expect(firstFindings(many, [], files, 2)).toHaveLength(2);
  });
});
