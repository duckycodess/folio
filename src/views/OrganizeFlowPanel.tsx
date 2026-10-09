import { ArrowRight, Copy, FolderOpen, Sparkles } from "lucide-react";
import { useEffect, useRef, type ReactNode, type RefObject } from "react";
import {
  suggestionKey,
  type OrganizeStage,
  type SuggestionKind,
} from "../app/organizeFlow";
import { planRow, summarizeApply, summarizeUndo } from "../app/planReview";
import type { CollectionsController } from "../app/useCollections";
import type { OrganizeController } from "../app/useOrganize";
import type { WorkspaceState } from "../app/useWorkspace";
import type {
  DocumentRecord,
  IndexProgress,
  SourcePassage,
} from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Notice } from "../ui/Notice";
import { Panel } from "../ui/Panel";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { PlanTable, UndoDialog } from "./PlanReview";
import { SuggestedCollections } from "./SuggestedCollections";

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

/**
 * Journey B: analyze a folder or one collection, then apply only the exact
 * plan approved. Analyzing the whole folder also suggests collections.
 */
export function OrganizeFlowPanel({
  workspace,
  organize,
  collections,
  onOpenFile,
}: {
  workspace: WorkspaceState;
  organize: OrganizeController;
  collections: CollectionsController;
  /** Opens a suggestion's or duplicate's file in the reader (#67). */
  onOpenFile: (document: DocumentRecord) => void;
}) {
  const { state } = organize;
  const heading = useRef<HTMLHeadingElement>(null);
  const target = collections.collections.find(
    (collection) => collection.id === organize.target,
  );

  // Bumped by Stop, so a run stopped during grouping doesn't go on to naming files.
  const modelRun = useRef(0);

  async function analyze() {
    const wholeFolder = !organize.target;
    if (wholeFolder) collections.clearSuggestions();
    const analyzed = await organize.analyze();
    if (!analyzed || !collections.available) return;
    // One generation at a time: collections first, then renames and moves.
    const run = ++modelRun.current;
    if (wholeFolder) await collections.suggest();
    if (run === modelRun.current) await organize.suggestWithModel();
  }

  function stopModels() {
    modelRun.current++;
    collections.stopSuggest();
    organize.stopAssist();
  }

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
                : () => void analyze(),
              previewAgain: organize.previewAgain,
            }}
            onDismiss={organize.dismissError}
          />
        )}

        {(state.stage === "idle" || state.stage === "suggestions") &&
          collections.collections.length > 0 && (
            <div className="organize-target">
              <label htmlFor="organize-target" className="field-label">
                What to analyze
              </label>
              <select
                id="organize-target"
                className="text-input"
                value={organize.target ?? ""}
                onChange={(event) =>
                  organize.setTarget(event.target.value || null)
                }
              >
                <option value="">The whole folder</option>
                {collections.collections.map((collection) => (
                  <option key={collection.id} value={collection.id}>
                    Collection: {collection.name}
                  </option>
                ))}
              </select>
            </div>
          )}

        {state.stage === "idle" && (
          <div className="flow-step">
            {target ? (
              <p>
                Folio re-reads {workspaceName(workspace)}, then suggests clearer
                file names for the files in “{target.name}” and shows which of
                them have identical copies. Nothing changes until you approve an
                exact preview.
              </p>
            ) : (
              <p>
                Folio re-reads {workspaceName(workspace)}, then suggests clearer
                file names and shows files with exactly the same contents. With
                a local AI model, it also groups files about the same material
                into collections you can keep. Nothing changes until you approve
                an exact preview.
              </p>
            )}
            <div className="form-actions">
              <Button variant="primary" onClick={() => void analyze()}>
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
                            {group.documents.map((document, index) => (
                              <span key={document.id}>
                                {index > 0 && ", "}
                                <button
                                  type="button"
                                  className="link-button plan-path"
                                  onClick={() => onOpenFile(document)}
                                >
                                  {document.relativePath}
                                </button>
                              </span>
                            ))}
                          </span>
                        </li>
                      ))}
                    </ul>
                  </>
                ) : (
                  <p className="muted">No files with identical contents.</p>
                )}
              </section>

              <FileChanges
                organize={organize}
                workspace={workspace}
                onOpenFile={onOpenFile}
                onRetry={() => void organize.suggestWithModel()}
                onStop={stopModels}
              />

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
                  <Button variant="ghost" onClick={() => void analyze()}>
                    Analyze again
                  </Button>
                </div>
              )}
              {!organize.target && (
                <SuggestedCollections
                  collections={collections}
                  onStop={stopModels}
                />
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

/**
 * Name and folder suggestions: title-based names, names the local model wrote,
 * and moves into folders whose files are closer in meaning. Each is an exact
 * native operation; at most one per file can be chosen for one preview.
 */
function FileChanges({
  organize,
  workspace,
  onOpenFile,
  onRetry,
  onStop,
}: {
  organize: OrganizeController;
  workspace: WorkspaceState;
  onOpenFile: (document: DocumentRecord) => void;
  onRetry: () => void;
  onStop: () => void;
}) {
  const { state } = organize;
  const { assist } = state;
  const result = assist.result;
  const titled = state.suggestions?.filenames ?? [];
  const named = result?.filenames ?? [];
  const moves = result?.destinations ?? [];
  const documentFor = (id: string) =>
    workspace.documents.find((candidate) => candidate.id === id);
  const row = (
    kind: SuggestionKind,
    item: {
      documentId: string;
      relativePath: string;
      suggestedRelativePath: string;
      reason: string;
    },
    extra?: ReactNode,
  ) => {
    const key = suggestionKey(kind, item.documentId);
    return (
      <SuggestionRow
        key={key}
        checked={state.chosen.includes(key)}
        onToggle={() => organize.toggle(key)}
        from={item.relativePath}
        to={item.suggestedRelativePath}
        reason={item.reason}
        document={documentFor(item.documentId)}
        onOpenFile={onOpenFile}
        extra={extra}
      />
    );
  };
  const several =
    new Set([...titled, ...named, ...moves].map((item) => item.documentId))
      .size <
    titled.length + named.length + moves.length;

  return (
    <>
      <fieldset className="suggestions">
        <legend className="subsection-title">Name suggestions</legend>
        {titled.length || named.length ? (
          <ul className="suggestion-list">
            {[
              ...titled.map((item) => ({ kind: "title" as const, item })),
              ...named.map((item) => ({ kind: "model" as const, item })),
            ]
              // Each file's suggestions side by side, title-based first.
              .sort(
                (a, b) =>
                  a.item.relativePath.localeCompare(b.item.relativePath) ||
                  (a.kind === "title" ? -1 : 1),
              )
              .map(({ kind, item }) =>
                row(
                  kind,
                  item,
                  item.generated && (
                    <>
                      <Badge>
                        <Sparkles size={12} aria-hidden="true" /> Name by local
                        AI
                      </Badge>
                      <Excerpt passages={item.generated.citations} />
                    </>
                  ),
                ),
              )}
          </ul>
        ) : (
          assist.status !== "working" && (
            <p className="muted">No name changes to suggest.</p>
          )
        )}
        {result?.naming === "generationModelMissing" &&
          result.filenameCandidates > 0 && (
            <p className="muted">
              {result.filenameCandidates}{" "}
              {result.filenameCandidates === 1
                ? "file has a generic name"
                : "files have generic names"}{" "}
              and no heading. Naming{" "}
              {result.filenameCandidates === 1 ? "it" : "them"} needs a local
              generation model.
            </p>
          )}
        {result?.naming === "cancelled" && (
          <p className="muted">Naming stopped; names already written stay.</p>
        )}
        {result?.naming === "failed" && (
          <Notice tone="warning">
            Folio couldn't name files with the local AI
            {result.namingError ? `: ${result.namingError.message}` : "."}
          </Notice>
        )}
      </fieldset>

      {assist.status !== "idle" && (
        <fieldset className="suggestions">
          <legend className="subsection-title">Folder suggestions</legend>
          {moves.length ? (
            <ul className="suggestion-list">
              {moves.map((item) =>
                row(
                  "move",
                  item,
                  <span className="muted collection-passage">
                    In “{item.folder || "the top of the folder"}”: “
                    {excerpt(item.evidence.text)}”
                  </span>,
                ),
              )}
            </ul>
          ) : result?.destinationStatus === "embeddingModelMissing" ? (
            <p className="muted">
              Folder suggestions need a local embedding model.
            </p>
          ) : (
            assist.status === "ready" && (
              <p className="muted">No folder changes to suggest.</p>
            )
          )}
        </fieldset>
      )}

      {several && (
        <p className="muted">
          Some files have more than one suggestion. Choose one change per file.
        </p>
      )}

      {assist.status === "working" && (
        <div className="flow-step">
          <Progress label="Asking the local AI for clearer names and folders" />
          <div className="form-actions">
            <Button variant="secondary" onClick={onStop}>
              Stop the local AI
            </Button>
          </div>
        </div>
      )}
      {assist.status === "failed" && assist.error && (
        <Notice
          tone="warning"
          action={
            <button type="button" className="link-button" onClick={onRetry}>
              Try again
            </button>
          }
        >
          Folio couldn't get name and folder suggestions: {assist.error.message}
        </Notice>
      )}
    </>
  );
}

function SuggestionRow({
  checked,
  onToggle,
  from,
  to,
  reason,
  document,
  onOpenFile,
  extra,
}: {
  checked: boolean;
  onToggle: () => void;
  from: string;
  to: string;
  reason: string;
  document: DocumentRecord | undefined;
  onOpenFile: (document: DocumentRecord) => void;
  extra?: ReactNode;
}) {
  return (
    <li>
      <label className="suggestion">
        <input type="checkbox" checked={checked} onChange={onToggle} />
        <span className="suggestion-text">
          <span className="suggestion-paths">
            {document ? (
              <button
                type="button"
                className="link-button plan-path"
                onClick={() => onOpenFile(document)}
              >
                {from}
              </button>
            ) : (
              <span className="plan-path">{from}</span>
            )}
            <ArrowRight size={14} aria-label="to" />
            <span className="plan-path">{to}</span>
          </span>
          <span className="muted">{reason}</span>
          {extra}
        </span>
      </label>
    </li>
  );
}

/** The first passage a model cited, shortened. */
function Excerpt({ passages }: { passages: SourcePassage[] }) {
  const first = passages[0];
  if (!first) return null;
  return (
    <span className="muted collection-passage">
      Based on “{excerpt(first.text)}”
      {first.page !== undefined && ` (page ${first.page})`}
    </span>
  );
}

function excerpt(text: string): string {
  const line = text.trim().split(/\r?\n/u).find(Boolean) ?? "";
  return line.length > 140 ? `${line.slice(0, 139)}…` : line;
}

/** The exact native plan, and Approve. Shared by Organize and Home's file actions. */
export function PreviewStep({
  organize,
  heading,
  cancelLabel,
  details,
}: {
  organize: OrganizeController;
  heading: RefObject<HTMLHeadingElement | null>;
  cancelLabel: string;
  /** More of the exact preview, such as a text diff and Ripple passages. */
  details?: ReactNode;
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
      {details}
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
