import { ArrowRight, Copy, FolderOpen } from "lucide-react";
import { useEffect, useRef, type RefObject } from "react";
import type { OrganizeStage } from "../app/organizeFlow";
import { planRow, summarizeApply, summarizeUndo } from "../app/planReview";
import type { OrganizeController } from "../app/useOrganize";
import type { WorkspaceState } from "../app/useWorkspace";
import type { IndexProgress } from "../domain/contracts";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Notice } from "../ui/Notice";
import { Panel } from "../ui/Panel";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { PlanTable, UndoDialog } from "./PlanReview";

const STEPS: { label: string; stages: OrganizeStage[] }[] = [
  { label: "Analyze", stages: ["idle", "analyzing"] },
  { label: "Choose suggestions", stages: ["suggestions", "preparing"] },
  { label: "Review the exact preview", stages: ["preview", "applying"] },
  { label: "Result", stages: ["result"] },
];

function progressLabel(progress: IndexProgress | null): {
  label: string;
  value?: number;
} {
  if (!progress) return { label: "Starting the analysis" };
  const counted = progress.total > 0;
  const value = counted
    ? (progress.processed / progress.total) * 100
    : undefined;
  switch (progress.phase) {
    case "discovering":
      return { label: "Finding files" };
    case "indexing":
      return {
        label: counted
          ? `Reading files (${progress.processed} of ${progress.total})`
          : "Reading files",
        value,
      };
    case "linking":
      return { label: "Finding links and duplicates", value };
    default:
      return { label: "Finishing up" };
  }
}

function Steps({ stage }: { stage: OrganizeStage }) {
  const current = STEPS.findIndex((step) => step.stages.includes(stage));
  return (
    <ol className="steps" aria-label="Steps">
      {STEPS.map((step, index) => (
        <li
          key={step.label}
          className={`step${index === current ? " is-current" : ""}${index < current ? " is-done" : ""}`}
          aria-current={index === current ? "step" : undefined}
        >
          <span className="step-number" aria-hidden="true">
            {index + 1}
          </span>
          {step.label}
        </li>
      ))}
    </ol>
  );
}

/** Journey B: analyze a folder, then apply only the exact plan approved. */
export function OrganizeFlowPanel({
  workspace,
  organize,
}: {
  workspace: WorkspaceState;
  organize: OrganizeController;
}) {
  const { state } = organize;
  const heading = useRef<HTMLHeadingElement>(null);

  // Each new step is announced by moving focus to its heading.
  useEffect(() => {
    if (state.stage === "preview" || state.stage === "result")
      heading.current?.focus();
  }, [state.stage]);

  if (workspace.source !== "folder") {
    return (
      <Panel title="Analyze a folder">
        <EmptyState
          // Collections below already shows Olio; one per view.
          icon={<FolderOpen size={24} />}
          title="Organizing works on your own folder"
          action={
            workspace.canChooseFolder && (
              <Button variant="primary" onClick={workspace.selectFolder}>
                Choose folder
              </Button>
            )
          }
        >
          {workspace.nativeAvailable
            ? "Add a folder to analyze it. Sample files can't be changed."
            : "Folder access works in the desktop app. Sample files can't be changed."}
        </EmptyState>
      </Panel>
    );
  }

  const suggestions = state.suggestions;
  const chosen = state.chosen.length;

  return (
    <Panel title="Analyze this folder">
      <div className="organize-flow">
        <Steps stage={state.stage} />

        {state.error && state.stage !== "preview" && (
          <RecoveryNotice
            error={state.error}
            actions={{
              retry: state.operations.length
                ? organize.previewAgain
                : organize.analyze,
              previewAgain: organize.previewAgain,
            }}
            onDismiss={organize.dismissError}
          />
        )}

        {state.stage === "idle" && (
          <div className="flow-step">
            <p>
              Folio re-reads {workspaceName(workspace)}, then suggests clearer
              file names and shows files with exactly the same contents. Nothing
              changes until you approve an exact preview.
            </p>
            <div className="form-actions">
              <Button variant="primary" onClick={organize.analyze}>
                Analyze
              </Button>
            </div>
          </div>
        )}

        {state.stage === "analyzing" && (
          <div className="flow-step">
            <Progress {...progressLabel(state.progress)} />
            <div className="form-actions">
              <Button variant="secondary" onClick={organize.cancelAnalyze}>
                Stop
              </Button>
            </div>
          </div>
        )}

        {(state.stage === "suggestions" || state.stage === "preparing") &&
          suggestions && (
            <div className="flow-step">
              <section aria-labelledby="duplicates-heading">
                <h3 id="duplicates-heading" className="subsection-title">
                  Exact duplicates
                </h3>
                {suggestions.duplicateGroups.length ? (
                  <>
                    <p className="muted">
                      These files have the same contents, byte for byte. Folio
                      doesn't move or delete them; decide which copy to keep.
                    </p>
                    <ul className="duplicate-groups">
                      {suggestions.duplicateGroups.map((group) => (
                        <li key={group.contentHash} className="duplicate-group">
                          <Copy size={16} aria-hidden="true" />
                          <span>
                            {group.documents.length} identical copies:{" "}
                            {group.documents
                              .map((document) => document.relativePath)
                              .join(", ")}
                          </span>
                        </li>
                      ))}
                    </ul>
                  </>
                ) : (
                  <p className="muted">No files with identical contents.</p>
                )}
              </section>

              <fieldset className="suggestions">
                <legend className="subsection-title">Name suggestions</legend>
                {suggestions.filenames.length ? (
                  <ul className="suggestion-list">
                    {suggestions.filenames.map((item) => (
                      <li key={item.documentId}>
                        <label className="suggestion">
                          <input
                            type="checkbox"
                            checked={state.chosen.includes(item.documentId)}
                            onChange={() => organize.toggle(item.documentId)}
                          />
                          <span className="suggestion-text">
                            <span className="suggestion-paths">
                              <span className="plan-path">
                                {item.relativePath}
                              </span>
                              <ArrowRight size={14} aria-label="to" />
                              <span className="plan-path">
                                {item.suggestedRelativePath}
                              </span>
                            </span>
                            <span className="muted">{item.reason}</span>
                          </span>
                        </label>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="muted">No name changes to suggest.</p>
                )}
              </fieldset>

              {state.stage === "preparing" ? (
                <Progress label="Preparing the exact preview" />
              ) : (
                <div className="form-actions">
                  <Button
                    variant="primary"
                    disabled={!chosen}
                    onClick={organize.previewChosen}
                  >
                    {chosen
                      ? `Preview ${chosen} ${chosen === 1 ? "change" : "changes"}`
                      : "Choose suggestions to preview"}
                  </Button>
                  <Button variant="ghost" onClick={organize.analyze}>
                    Analyze again
                  </Button>
                </div>
              )}
            </div>
          )}

        {state.stage === "preparing" && !suggestions && (
          <Progress label="Preparing the exact preview" />
        )}

        {(state.stage === "preview" || state.stage === "applying") && (
          <PreviewStep
            organize={organize}
            heading={heading}
            cancelLabel={suggestions ? "Back to suggestions" : "Cancel"}
          />
        )}

        {state.stage === "result" && state.plan && state.report && (
          <ResultStep organize={organize} heading={heading} />
        )}
      </div>
    </Panel>
  );
}

/** The exact native plan, and Approve. Shared by Organize and Home's file actions. */
export function PreviewStep({
  organize,
  heading,
  cancelLabel,
}: {
  organize: OrganizeController;
  heading: RefObject<HTMLHeadingElement | null>;
  cancelLabel: string;
}) {
  const { state } = organize;
  if (!state.plan) return null;
  return (
    <div className="flow-step">
      <h3 ref={heading} tabIndex={-1} className="subsection-title">
        Exact preview: nothing has changed yet
      </h3>
      <PlanTable
        rows={state.plan.operations.map(planRow)}
        caption="Changes Folio will make after you approve"
      />
      {state.plan.impacts.length > 0 && (
        <Notice tone="info">
          {state.plan.impacts.length} related{" "}
          {state.plan.impacts.length === 1 ? "passage" : "passages"} may need a
          look afterwards. They won't be changed.
        </Notice>
      )}
      {state.error && (
        <RecoveryNotice
          error={state.error}
          actions={{ previewAgain: organize.previewAgain }}
          onDismiss={organize.dismissError}
        />
      )}
      {state.stage === "applying" ? (
        <Progress label="Applying the approved changes" />
      ) : (
        <div className="form-actions">
          <Button
            variant="primary"
            disabled={state.error !== null}
            onClick={organize.approveAndApply}
          >
            Approve and apply{" "}
            {state.plan.operations.length === 1
              ? "this change"
              : `${state.plan.operations.length} changes`}
          </Button>
          <Button variant="ghost" onClick={organize.backToSuggestions}>
            {cancelLabel}
          </Button>
        </div>
      )}
    </div>
  );
}

/** What happened, with history and Undo. Shared with Home's file actions. */
export function ResultStep({
  organize,
  heading,
  onDone = organize.done,
}: {
  organize: OrganizeController;
  heading: RefObject<HTMLHeadingElement | null>;
  /** Defaults to starting the flow again; a dialog closes instead. */
  onDone?: () => void;
}) {
  const { state, undo, history } = organize;
  const summary = summarizeApply(state.plan!, state.report!);
  const undoResult = undo.report && summarizeUndo(undo.report);

  return (
    <div className="flow-step">
      <h3 ref={heading} tabIndex={-1} className="subsection-title">
        {summary.headline}
      </h3>
      {summary.details.length > 0 && (
        <Notice tone={summary.tone === "saved" ? "info" : "warning"}>
          {summary.details.join(" ")}
        </Notice>
      )}
      <PlanTable rows={summary.rows} caption="What happened to each change" />
      {history.length > 0 && (
        <p className="muted">
          Recorded in history: {history.length}{" "}
          {history.length === 1 ? "entry" : "entries"}
          {history.some((entry) => !entry.recoverable) &&
            ", some without a way to undo"}
          .
        </p>
      )}
      {undoResult && (
        <Notice tone={undoResult.complete ? "info" : "warning"}>
          {undoResult.headline}
        </Notice>
      )}
      {undo.error && (
        <RecoveryNotice
          error={undo.error}
          stage={undo.report?.undoneEntryIds.length ? "partialUndo" : "refused"}
          actions={{ previewAgain: organize.previewUndo }}
        />
      )}
      <div className="form-actions">
        {summary.undoable && !undoResult?.complete && (
          <Button
            variant="secondary"
            disabled={undo.busy}
            onClick={organize.previewUndo}
          >
            Preview Undo
          </Button>
        )}
        <Button variant="ghost" onClick={onDone}>
          Done
        </Button>
      </div>

      <UndoDialog
        preflight={undo.preview}
        busy={undo.busy}
        onConfirm={organize.confirmUndo}
        onClose={organize.closeUndo}
      />
    </div>
  );
}

function workspaceName(workspace: WorkspaceState): string {
  const root = workspace.workspace?.rootPath ?? "";
  return `“${root.split(/[\\/]/).filter(Boolean).at(-1) ?? root}”`;
}
