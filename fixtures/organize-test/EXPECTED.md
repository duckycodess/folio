# Organize test workspace: expected suggestions

Point Folio at `fixtures/organize-test/workspace/` (not this folder, so this file stays
out of the index), then run Analyze in Organize. Content is fictional and mixes English,
Filipino, and Taglish.

## Exact duplicates (byte-identical, matched by content hash)

| Group | Members |
| --- | --- |
| Quarterly Report | backup/quarterly-report-copy.md, downloads/quarterly-report (1).md, reports/quarterly-report.md |
| Sinigang na Baboy | backup/sinigang na baboy (old).md, resipe/sinigang-na-baboy.md |

Duplicates are evidence only. Nothing is moved or deleted unless the user builds and
approves a plan.

## Not duplicates

- budget/budget.md and budget/budget-v2.md share a title and differ by one line item and
  the total. They must not form a duplicate group.
- misc/meeting.md and misc/project-kickoff.md share the title "Project Kickoff" but have
  different content.

## Filename suggestions (name differs from the slug of the title)

| Current | Suggested |
| --- | --- |
| backup/quarterly-report-copy.md | backup/quarterly-report.md |
| backup/sinigang na baboy (old).md | backup/sinigang-na-baboy.md |
| budget/budget-v2.md | budget/org-fair-budget.md |
| downloads/IMG_scan_final_FINAL.md | downloads/barangay-clean-up-volunteer-schedule.md |
| downloads/Untitled document (3).md | downloads/kuwaderno-sa-kimika-ikalawang-markahan.md |
| downloads/quarterly-report (1).md | downloads/quarterly-report.md |
| notes/doc.md | notes/listahan-ng-babasahin-para-sa-thesis.md |
| notes/notes1.txt | notes/group-4-meeting-recap.txt (title from the first line; no heading) |

## No suggestion expected

- reports/quarterly-report.md, resipe/sinigang-na-baboy.md, misc/project-kickoff.md:
  already named after their titles.
- budget/budget.md: its slug `org-fair-budget.md` is already claimed by budget-v2.md,
  which sorts first.
- misc/meeting.md: `misc/project-kickoff.md` already exists, so the rename would collide.
- misc/very-long-title.md: the title's slug is longer than 60 characters.
- misc/symbols.md: the heading `# !!!` produces an empty slug.

Approving a rename and then trying the skipped collision manually exercises the
rename-collision and stale-approval checks.
