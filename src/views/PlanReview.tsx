import {
  Fragment,
  useEffect,
  useId,
  useMemo,
  useRef,
  type RefObject,
} from "react";
import { hasUndoableChange } from "../app/planAction";
import {
  deletionImpactNote,
  impactBadge,
  impactGroups,
  impactProvenance,
  planRow,
  RIPPLE_CANDIDATE_CAP,
  summarizeApply,
  summarizeUndo,
  undoBlockers,
  type ImpactKind,
  type PlanRow,
} from "../app/planReview";
import type { PlanActionController } from "../app/usePlanAction";
import type {
  ActionPlan,
  ApplyReport,
  DocumentId,
  FileOperation,
  ImpactCandidate,
  UndoPreflight,
} from "../domain/contracts";
import type { FolioError } from "../domain/errors";
import { diffLines, type DiffLine } from "../domain/textDiff";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { Notice } from "../ui/Notice";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";

/*
 * The exact preview → approve → result → Undo pieces every file change shares:
 * Organize, the file action dialogs and Ask & Act.
 */

const OUTCOME_LABELS: Record<string, string> = {
  succeeded: "Saved",
  failed: "Not saved",
  cancelled: "Stopped",
  notStarted: "Not started",
};

/** Every change in a plan, from → to, with each outcome once there is one. */
export function PlanTable({
  rows,
  caption,
}: {
  rows: (PlanRow & { status?: string; reason?: string })[];
  caption: string;
}) {
  const withStatus = rows.some((row) => row.status);
  return (
    <div className="plan-table-wrap">
      <table className="plan-table">
        <caption className="visually-hidden">{caption}</caption>
        <thead>
          <tr>
            <th scope="col">Change</th>
            <th scope="col">From</th>
            <th scope="col">To</th>
            {withStatus && <th scope="col">Outcome</th>}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr key={`${row.from}-${row.to}-${index}`}>
              <td>{row.action}</td>
              <td className="plan-path">{row.from ?? "New file"}</td>
              <td className="plan-path">{row.to ?? "Removed"}</td>
              {withStatus && (
                <td>
                  {OUTCOME_LABELS[row.status ?? "notStarted"]}
                  {row.reason && (
                    <span className="plan-reason">{row.reason}</span>
                  )}
                </td>
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const DIFF_LABELS: Record<DiffLine["kind"], { marker: string; label: string }> =
  {
    added: { marker: "+", label: "Added" },
    removed: { marker: "−", label: "Removed" },
    same: { marker: "", label: "Unchanged" },
  };

/**
 * The exact text change, line by line. Markers and spoken labels carry the
 * meaning, so color is never the only signal. A change too large to compare
 * shows the full new text instead, so the preview is always exact.
 */
export function TextDiffView({
  path,
  before,
  after,
}: {
  path: string;
  before: string;
  after: string;
}) {
  const diff = useMemo(() => diffLines(before, after), [before, after]);

  if (diff.tooLarge)
    return (
      <div className="diff">
        <Notice tone="info">
          This change is too large to show line by line. Below is the full text
          that will be saved to {path}.
        </Notice>
        <pre className="source-text diff-full">{after}</pre>
      </div>
    );
  if (!diff.hunks.length)
    return <p className="muted">The text of {path} is unchanged.</p>;

  return (
    <div className="diff">
      <table className="diff-table">
        <caption className="visually-hidden">Text changes in {path}</caption>
        <thead>
          <tr>
            <th scope="col">Before</th>
            <th scope="col">After</th>
            <th scope="col">
              <span className="visually-hidden">Change</span>
            </th>
            <th scope="col">Text</th>
          </tr>
        </thead>
        <tbody>
          {diff.hunks.map((hunk, index) => (
            <Fragment key={index}>
              {index > 0 && (
                <tr className="diff-gap">
                  <td colSpan={4}>Unchanged lines not shown</td>
                </tr>
              )}
              {hunk.lines.map((line) => {
                const { marker, label } = DIFF_LABELS[line.kind];
                return (
                  <tr
                    key={`${line.beforeLine ?? ""}-${line.afterLine ?? ""}`}
                    className={`diff-line diff-${line.kind}`}
                  >
                    <td className="diff-number">{line.beforeLine}</td>
                    <td className="diff-number">{line.afterLine}</td>
                    <td className="diff-marker">
                      <span aria-hidden="true">{marker}</span>
                      <span className="visually-hidden">{label}</span>
                    </td>
                    <td className="diff-text">
                      {line.text}
                      {line.kind !== "same" && !line.ending && (
                        <span className="diff-note">
                          {" "}
                          (no line break at the end)
                        </span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </Fragment>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const IMPACT_HEADINGS: Record<ImpactKind, string> = {
  links: "Linked files",
  copies: "Identical copies",
  inferred: "Found by the local AI",
  other: "Other related files",
};

/** For a deletion: what stops working, and what stays. */
const DELETION_HEADINGS: Record<ImpactKind, string> = {
  links: "Links that will stop working",
  copies: "Identical copies, which stay",
  inferred: "Related files the local AI found",
  other: "Other related files",
};

/**
 * Folio Ripple: related passages that may need a look. They are review
 * candidates only; this list never says a file was or will be updated.
 */
export function ImpactList({
  impacts,
  deletion = false,
}: {
  impacts: ImpactCandidate[];
  /** The plan deletes a file: say which links break and what stays. */
  deletion?: boolean;
}) {
  const groups = impactGroups(impacts);
  const headingId = useId();
  const headings = deletion ? DELETION_HEADINGS : IMPACT_HEADINGS;
  return (
    <section className="impact-review" aria-labelledby={headingId}>
      <h3 id={headingId} className="subsection-title">
        {deletion ? "What this deletion affects" : "Related passages to review"}
      </h3>
      <p className="muted">
        {deletion
          ? deletionImpactNote(groups)
          : impacts.length === 0
            ? "Folio didn't find related files that mention what you changed. That doesn't guarantee nothing else needs a look."
            : "Folio won't change these files. Check them yourself after saving."}
      </p>
      {(Object.keys(headings) as ImpactKind[]).map(
        (kind) =>
          groups[kind].length > 0 && (
            <div key={kind} className="impact-group">
              <h4 className="impact-group-title">{headings[kind]}</h4>
              <ul className="impact-list">
                {groups[kind].map((impact) => (
                  <ImpactItem
                    key={impact.documentId}
                    impact={impact}
                    badge={impactBadge(impact, deletion)}
                  />
                ))}
              </ul>
            </div>
          ),
      )}
      {impacts.length >= RIPPLE_CANDIDATE_CAP && (
        <p className="muted">
          Folio lists at most {RIPPLE_CANDIDATE_CAP} related files, so there may
          be more.
        </p>
      )}
    </section>
  );
}

function ImpactItem({
  impact,
  badge,
}: {
  impact: ImpactCandidate;
  badge: string | null;
}) {
  const provenance = impactProvenance(impact);
  return (
    <li className="impact">
      <div className="impact-head">
        {badge && <Badge>{badge}</Badge>}
        {provenance.ai && <Badge>AI</Badge>}
        <span className="plan-path">{impact.relativePath}</span>
      </div>
      <p>{impact.reason}</p>
      <p className="impact-provenance">{provenance.label}</p>
      {impact.evidence.length > 0 && (
        <ul className="impact-passages" aria-label="Passages">
          {impact.evidence.map((passage) => (
            <li key={`${passage.start}-${passage.end}`}>
              <blockquote className="impact-passage">{passage.text}</blockquote>
              {passage.page !== undefined && (
                <span className="muted">Page {passage.page}</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

/** What happened to an approved plan, worded from the native report alone. */
export function ApplyResult({
  plan,
  report,
  heading,
}: {
  plan: ActionPlan;
  report: ApplyReport;
  heading?: RefObject<HTMLHeadingElement | null>;
}) {
  const summary = summarizeApply(plan, report);
  return (
    <>
      <h3 ref={heading} tabIndex={-1} className="subsection-title">
        {summary.headline}
      </h3>
      {summary.details.length > 0 && (
        <Notice tone={summary.tone === "saved" ? "info" : "warning"}>
          {summary.details.join(" ")}
        </Notice>
      )}
      <PlanTable rows={summary.rows} caption="What happened to each change" />
    </>
  );
}

/** What Undo would do, or what blocks it. Nothing changes until confirmed. */
export function UndoSummary({ preflight }: { preflight: UndoPreflight }) {
  const blockers = undoBlockers(preflight);
  return (
    <>
      {blockers.length ? (
        <>
          <p>Folio can't undo safely, so nothing will be changed:</p>
          <ul>
            {blockers.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </>
      ) : (
        <p>
          Folio will put{" "}
          {preflight.entryIds.length === 1 ? "this file" : "these files"} back
          the way they were before you approved.
        </p>
      )}
      <Badge>Nothing changes until you confirm</Badge>
    </>
  );
}

function undoLabel(preflight: UndoPreflight | null): string {
  const count = preflight?.entryIds.length ?? 0;
  return `Undo ${count} ${count === 1 ? "change" : "changes"}`;
}

/** Undo's confirmation as a modal, for pages that aren't already one. */
export function UndoDialog({
  preflight,
  busy,
  error,
  onConfirm,
  onClose,
  onPreviewAgain,
}: {
  preflight: UndoPreflight | null;
  busy: boolean;
  /** A refusal of this Undo, shown inside the dialog. */
  error?: FolioError | null;
  onConfirm: () => void;
  onClose: () => void;
  onPreviewAgain?: () => void;
}) {
  return (
    <Modal
      open={preflight !== null}
      title="Undo these changes?"
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Keep the changes
          </Button>
          <Button
            variant="primary"
            disabled={!preflight?.undoable || busy || Boolean(error)}
            onClick={onConfirm}
          >
            {undoLabel(preflight)}
          </Button>
        </>
      }
    >
      {preflight && <UndoSummary preflight={preflight} />}
      {error && (
        <RecoveryNotice
          error={error}
          actions={onPreviewAgain ? { previewAgain: onPreviewAgain } : {}}
        />
      )}
    </Modal>
  );
}

/** Preview the change again, or go back to change what was asked for. */
interface ReviewActions {
  /** Replaces the default "Preview again", e.g. to re-read a changed file first. */
  onPreviewAgain?: () => void;
  /** Back to the form that produced the plan. */
  onBack: () => void;
  onDone: () => void;
}

/**
 * The whole review for one plan: exact preview, apply, result and Undo, from
 * a `usePlanAction` controller. Inside a modal, Undo is confirmed in place
 * rather than in a second modal.
 */
export function PlanReview({
  action,
  beforeText = {},
  approveLabel,
  backLabel = "Back",
  inModal = false,
  onPreviewAgain,
  onBack,
  onDone,
}: ReviewActions & {
  action: PlanActionController;
  /** The text each edited document had when the edit was made, for its diff. */
  beforeText?: Record<DocumentId, string>;
  approveLabel?: string;
  backLabel?: string;
  inModal?: boolean;
}) {
  const { state } = action;
  const heading = useRef<HTMLHeadingElement>(null);
  const previewAgain = onPreviewAgain ?? action.previewAgain;
  const reviewing = state.stage === "preview" || state.stage === "applying";

  // Each new step is announced by moving focus to its heading.
  useEffect(() => {
    if (state.stage === "preview" || state.stage === "result")
      heading.current?.focus();
  }, [state.stage]);

  if (state.stage === "idle")
    return (
      state.error && (
        <RecoveryNotice
          error={state.error}
          actions={{
            previewAgain,
            retry: previewAgain,
            chooseAnotherName: onBack,
          }}
          onDismiss={action.dismissError}
        />
      )
    );

  if (state.stage === "preparing")
    return <Progress label="Preparing the exact preview" />;

  const plan = state.plan;
  if (!plan) return null;

  if (reviewing) {
    const edits = plan.operations.filter(
      (operation): operation is Extract<FileOperation, { kind: "edit" }> =>
        operation.kind === "edit",
    );
    const count = plan.operations.length;
    return (
      <div className="flow-step">
        <h3 ref={heading} tabIndex={-1} className="subsection-title">
          Exact preview: nothing has changed yet
        </h3>
        <PlanTable
          rows={plan.operations.map(planRow)}
          caption="Changes Folio will make after you approve"
        />
        {edits.map((edit) =>
          beforeText[edit.documentId] !== undefined ? (
            <TextDiffView
              key={edit.documentId}
              path={edit.relativePath}
              before={beforeText[edit.documentId]}
              after={edit.after}
            />
          ) : (
            <Fragment key={edit.documentId}>
              <p className="muted">Full new text of {edit.relativePath}:</p>
              <pre className="source-text diff-full">{edit.after}</pre>
            </Fragment>
          ),
        )}
        {(edits.length > 0 || plan.impacts.length > 0) && (
          <ImpactList impacts={plan.impacts} />
        )}
        {state.error && (
          <RecoveryNotice
            error={state.error}
            actions={{ previewAgain, chooseAnotherName: onBack }}
            onDismiss={action.dismissError}
          />
        )}
        {state.stage === "applying" ? (
          <Progress label="Saving the approved change" />
        ) : (
          <div className="form-actions">
            <Button
              variant="primary"
              disabled={state.error !== null}
              onClick={action.approveAndApply}
            >
              {approveLabel ??
                `Approve and apply ${count === 1 ? "this change" : `${count} changes`}`}
            </Button>
            <Button variant="ghost" onClick={onBack}>
              {backLabel}
            </Button>
          </div>
        )}
      </div>
    );
  }

  const report = state.report;
  if (!report) return null;
  const undoResult = state.undoReport && summarizeUndo(state.undoReport);
  const undoing = state.stage === "undoPreview" || state.stage === "undoing";
  const undoError =
    state.error &&
    (state.stage === "undoPreview" ||
      state.stage === "result" ||
      state.stage === "undone")
      ? state.error
      : null;
  const canUndo =
    hasUndoableChange(report) && !undoResult?.complete && !undoing;

  return (
    <div className="flow-step">
      <ApplyResult plan={plan} report={report} heading={heading} />
      {undoResult && (
        <Notice tone={undoResult.complete ? "info" : "warning"}>
          {undoResult.headline}
        </Notice>
      )}
      {state.undoReport?.error && (
        <RecoveryNotice
          error={state.undoReport.error}
          stage={
            state.undoReport.undoneEntryIds.length ? "partialUndo" : "refused"
          }
          actions={{ previewAgain: action.previewUndo }}
        />
      )}
      {inModal && undoing && state.undoPreflight && (
        <section className="undo-confirm" aria-label="Undo">
          <UndoSummary preflight={state.undoPreflight} />
          {undoError && (
            <RecoveryNotice
              error={undoError}
              actions={{ previewAgain: action.previewUndo }}
            />
          )}
          {state.stage === "undoing" ? (
            <Progress label="Undoing the change" />
          ) : (
            <div className="form-actions">
              <Button
                variant="primary"
                disabled={!state.undoPreflight.undoable || Boolean(undoError)}
                onClick={action.confirmUndo}
              >
                {undoLabel(state.undoPreflight)}
              </Button>
              <Button variant="ghost" onClick={action.closeUndo}>
                Keep the changes
              </Button>
            </div>
          )}
        </section>
      )}
      {!undoing && undoError && (
        <RecoveryNotice
          error={undoError}
          actions={{ previewAgain: action.previewUndo }}
          onDismiss={action.dismissError}
        />
      )}
      {state.stage === "previewingUndo" && (
        <Progress label="Checking whether Undo is safe" />
      )}
      {!undoing && state.stage !== "previewingUndo" && (
        <div className="form-actions">
          {canUndo && (
            <Button variant="secondary" onClick={action.previewUndo}>
              Preview Undo
            </Button>
          )}
          <Button variant="ghost" onClick={onDone}>
            Done
          </Button>
        </div>
      )}
      {!inModal && (
        <UndoDialog
          preflight={undoing ? state.undoPreflight : null}
          busy={state.stage === "undoing"}
          error={undoError}
          onConfirm={action.confirmUndo}
          onClose={action.closeUndo}
          onPreviewAgain={action.previewUndo}
        />
      )}
    </div>
  );
}
