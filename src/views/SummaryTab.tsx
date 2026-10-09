import { Sparkles } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { isAvailable as aiAvailable } from "../adapters/ai";
import {
  coveredPercent,
  isStale,
  summaryMarkdown,
  summaryPath,
} from "../app/summaries";
import { useOrganize } from "../app/useOrganize";
import type { RelationshipsState } from "../app/useRelationships";
import { cancelSummary, summarize, useSummary } from "../app/useSummary";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord, GroundedResult } from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Modal } from "../ui/Modal";
import { Notice } from "../ui/Notice";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { CitedSentences } from "./CitedSentences";
import { PreviewStep, ResultStep } from "./OrganizeFlowPanel";

/**
 * Save a summary as a new Markdown file next to its source: the exact file
 * contents first, then the native preview and approval, then the result.
 */
function SaveSummaryDialog({
  document,
  result,
  madeAt,
  workspace,
  relations,
  onClose,
}: {
  document: DocumentRecord;
  result: GroundedResult;
  madeAt: number;
  workspace: WorkspaceState;
  relations: RelationshipsState;
  onClose: () => void;
}) {
  const organize = useOrganize(workspace, relations.refresh);
  const { state } = organize;
  const heading = useRef<HTMLHeadingElement>(null);
  const [path] = useState(() =>
    summaryPath(
      document.relativePath,
      workspace.documents.map((item) => item.relativePath),
    ),
  );
  const [content] = useState(() =>
    summaryMarkdown(document, result, new Date(madeAt)),
  );
  const inPlan = state.stage === "preview" || state.stage === "applying";

  useEffect(() => {
    if (state.stage === "preview" || state.stage === "result")
      heading.current?.focus();
  }, [state.stage]);

  return (
    <Modal
      open
      title="Save summary as a new document"
      onClose={() => {
        if (state.stage !== "applying") organize.done();
        onClose();
      }}
    >
      {state.stage === "result" ? (
        <ResultStep organize={organize} heading={heading} />
      ) : inPlan ? (
        <PreviewStep organize={organize} heading={heading} cancelLabel="Back" />
      ) : (
        <div className="flow-step">
          <p>
            Folio will create <strong>{path}</strong> with exactly this text.
            The original file isn't changed.
          </p>
          <pre className="source-text summary-file-preview">{content}</pre>
          {state.error && (
            <RecoveryNotice
              error={state.error}
              actions={{
                retry: () => organize.previewCreate(path, content),
                previewAgain: () => organize.previewCreate(path, content),
              }}
              onDismiss={organize.dismissError}
            />
          )}
          {state.stage === "preparing" ? (
            <Progress label="Preparing the exact preview" />
          ) : (
            <div className="form-actions">
              <Button
                variant="primary"
                onClick={() => organize.previewCreate(path, content)}
              >
                Preview new file
              </Button>
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}

function SummaryResult({
  document,
  result,
  madeAt,
  workspace,
  relations,
  onAgain,
  onClear,
}: {
  document: DocumentRecord;
  result: GroundedResult;
  madeAt: number;
  workspace: WorkspaceState;
  relations: RelationshipsState;
  onAgain: () => void;
  onClear: () => void;
}) {
  const [saving, setSaving] = useState(false);
  const stale = isStale(result, document);
  const covered = coveredPercent(result, document);

  if (result.kind === "insufficientEvidence")
    return (
      <div className="summary">
        <Notice tone="info">
          There isn't enough information in this file for a summary.
          {result.text && ` ${result.text}`}
        </Notice>
        <div className="form-actions">
          <Button onClick={onAgain}>Summarize again</Button>
        </div>
      </div>
    );

  return (
    <div className="summary">
      <div className="summary-head">
        <Badge>
          {result.kind === "partialSummary" ? "Partial summary" : "Summary"}
        </Badge>
        <Badge>Generated preview, not saved</Badge>
      </div>
      <p className="muted">
        Made by the local model {result.modelId} (revision{" "}
        {result.revision.slice(0, 12)}). Not reviewed for accuracy: each point
        links to the passage it came from.
        {result.kind === "partialSummary" &&
          (covered !== null
            ? ` It covers about ${covered}% of the file.`
            : " It covers only part of the file.")}
      </p>
      {stale && (
        <Notice tone="warning">
          This file changed after the summary was made, so its sources may no
          longer match.
        </Notice>
      )}
      <CitedSentences result={result} onOpen={relations.openPassage} />
      <div className="form-actions">
        <Button variant="primary" onClick={() => setSaving(true)}>
          Save as new document…
        </Button>
        <Button onClick={onAgain}>Summarize again</Button>
        <Button variant="ghost" onClick={onClear}>
          Clear
        </Button>
      </div>
      {saving && (
        <SaveSummaryDialog
          document={document}
          result={result}
          madeAt={madeAt}
          workspace={workspace}
          relations={relations}
          onClose={() => setSaving(false)}
        />
      )}
    </div>
  );
}

/**
 * A file's Summary tab: the only place its summary appears, whether it was
 * started here or from Ask & Act. Nothing is generated until asked.
 */
export function SummaryTab({
  document,
  workspace,
  relations,
  onNavigate,
}: {
  document: DocumentRecord;
  workspace: WorkspaceState;
  relations: RelationshipsState;
  onNavigate: (view: ViewId) => void;
}) {
  const { entry, busyElsewhere, clear } = useSummary(document.id);
  const folderId =
    workspace.source === "folder" ? workspace.workspace?.id : undefined;

  if (!aiAvailable())
    return (
      <EmptyState
        icon={<Sparkles size={24} />}
        title="Summaries need the desktop app"
        action={
          <Button onClick={() => onNavigate("modelLab")}>Open Model Lab</Button>
        }
      >
        Folio summarizes with a local AI model on your computer. You can still
        read the whole file in Details.
      </EmptyState>
    );

  if (!folderId)
    return (
      <EmptyState
        icon={<Sparkles size={24} />}
        title="Summaries work on your own folder"
      >
        Sample files can't be summarized. Add a folder to summarize its files.
      </EmptyState>
    );

  const start = () => void summarize(folderId, document.id);

  if (entry?.status === "running")
    return (
      <div className="summary">
        <Progress label={`Summarizing ${document.name}`} />
        <p className="muted">
          You can keep browsing. The summary appears here when it's ready.
        </p>
        <div className="form-actions">
          <Button onClick={() => void cancelSummary()}>Cancel</Button>
        </div>
      </div>
    );

  if (entry?.status === "done")
    return (
      <SummaryResult
        document={document}
        result={entry.result}
        madeAt={entry.madeAt}
        workspace={workspace}
        relations={relations}
        onAgain={start}
        onClear={clear}
      />
    );

  return (
    <div className="summary">
      {entry?.status === "cancelled" && (
        <Notice tone="info">Summary cancelled. Nothing was saved.</Notice>
      )}
      {entry?.status === "failed" && (
        <RecoveryNotice
          error={entry.error}
          actions={{
            retry: start,
            openModelLab: () => onNavigate("modelLab"),
          }}
          onDismiss={clear}
        />
      )}
      <EmptyState
        icon={<Sparkles size={24} />}
        title="No summary yet"
        action={
          <Button variant="primary" disabled={busyElsewhere} onClick={start}>
            Summarize this file
          </Button>
        }
      >
        {busyElsewhere
          ? "Another file is being summarized. One summary runs at a time."
          : "The local AI model reads this file on your computer and links each point to its source passage. Nothing is saved unless you ask."}
      </EmptyState>
    </div>
  );
}
