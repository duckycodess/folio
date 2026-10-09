import { ArrowRight, History, Undo2 } from "lucide-react";
import type { ActivityState } from "../app/useActivity";
import type { WorkspaceState } from "../app/useWorkspace";
import {
  batchTitle,
  changeKind,
  CONFLICT_REASONS,
  STATUS_LABELS,
  type ActivityBatch,
} from "../domain/activity";
import type { HistoryEntry } from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Modal } from "../ui/Modal";
import { Notice } from "../ui/Notice";
import { Panel } from "../ui/Panel";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { formatModified } from "./format";

/** Files listed per entry before "and N more". */
const FILES_SHOWN = 5;

/**
 * Activity: what Folio actually changed on disk, newest first (#34). Every
 * entry comes from the native history; previews and analyses never appear.
 */
export function ActivityView({
  workspace,
  activity,
}: {
  workspace: WorkspaceState;
  activity: ActivityState;
}) {
  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Activity</h1>
        <p className="page-tagline">
          Every change Folio made to your files, newest first.
        </p>
      </header>
      {activity.undoResult.status === "succeeded" && (
        <Notice
          tone="success"
          action={
            <button
              type="button"
              className="link-button"
              onClick={activity.dismissUndoResult}
            >
              Dismiss
            </button>
          }
        >
          Undone.{" "}
          {activity.undoResult.result.undoneEntryIds.length === 1
            ? "1 file is back as it was."
            : `${activity.undoResult.result.undoneEntryIds.length} files are back as they were.`}
        </Notice>
      )}
      <Panel title="Changes" actions={<Badge>Folio's changes only</Badge>}>
        <ActivityBody workspace={workspace} activity={activity} />
      </Panel>
      <UndoDialog activity={activity} />
    </div>
  );
}

function ActivityBody({
  workspace,
  activity,
}: {
  workspace: WorkspaceState;
  activity: ActivityState;
}) {
  if (workspace.source !== "folder")
    return (
      <EmptyState icon={<History size={24} />} title="No activity yet">
        {workspace.source === "samples"
          ? "Folio never changes the sample files, so there's nothing to show."
          : "Add a folder. Changes Folio makes there, with your approval, appear here."}
      </EmptyState>
    );
  if (activity.status === "loading")
    return <p className="muted">Loading activity…</p>;
  if (activity.status === "failed" && activity.failure)
    return (
      <RecoveryNotice
        error={activity.failure}
        actions={{ retry: activity.reload }}
      />
    );
  if (!activity.batches.length)
    return (
      <EmptyState
        icon={<History size={24} />}
        title="Nothing has been changed by Folio yet"
      >
        Renames and moves you approve in Organize or Ask &amp; Act appear here,
        with Undo when it's safe.
      </EmptyState>
    );
  return (
    <>
      <ol className="activity-list" aria-label="Changes, newest first">
        {activity.batches.map((batch) => (
          <ActivityItem
            key={batch.planId}
            batch={batch}
            onUndo={() => activity.startUndo(batch)}
          />
        ))}
      </ol>
      <p className="muted">
        {activity.truncated &&
          "Only the most recent changes are shown, and the oldest entry may be incomplete. "}
        Failed and cancelled attempts aren't recorded yet, and neither is which
        page started a change.
      </p>
    </>
  );
}

function ActivityItem({
  batch,
  onUndo,
}: {
  batch: ActivityBatch;
  onUndo: () => void;
}) {
  const title = batchTitle(batch);
  const shown = batch.entries.slice(0, FILES_SHOWN);
  const hidden = batch.entries.length - shown.length;
  return (
    <li className="activity-item">
      <div className="activity-head">
        <div className="activity-heading">
          <h2 className="activity-title">{title}</h2>
          <p className="activity-meta">
            <time dateTime={new Date(batch.appliedAt).toISOString()}>
              {formatModified(batch.appliedAt)}
            </time>
          </p>
        </div>
        <Badge>{STATUS_LABELS[batch.status]}</Badge>
        {batch.canUndo && (
          <Button
            icon={<Undo2 size={16} />}
            onClick={onUndo}
            aria-label={`Undo: ${title}`}
          >
            Undo
          </Button>
        )}
      </div>
      <ul className="activity-files">
        {shown.map((entry) => (
          <li key={entry.id}>
            <ChangeLine entry={entry} />
          </li>
        ))}
      </ul>
      {hidden > 0 && (
        <p className="muted">
          and {hidden} more {hidden === 1 ? "file" : "files"}
        </p>
      )}
      {!batch.canUndo && batch.status !== "undone" && (
        <p className="muted">
          Undo isn't available: Folio couldn't keep the earlier version.
        </p>
      )}
    </li>
  );
}

function ChangeLine({ entry }: { entry: HistoryEntry }) {
  const kind = changeKind(entry);
  const before = entry.beforeRelativePath;
  const after = entry.afterRelativePath;
  return (
    <span className={`change-line${entry.undoneAt ? " is-undone" : ""}`}>
      {kind === "create" ? (
        <>Created {after}</>
      ) : kind === "delete" ? (
        <>Deleted {before}</>
      ) : kind === "edit" ? (
        <>Edited {after}</>
      ) : (
        <>
          <span>{before}</span>
          <ArrowRight size={14} aria-label="became" />
          <span>{after}</span>
        </>
      )}
      {entry.undoneAt !== undefined && <span className="muted"> (undone)</span>}
    </span>
  );
}

/** Shows exactly what Undo will restore, or why it can't, before anything changes. */
function UndoDialog({ activity }: { activity: ActivityState }) {
  const batch = activity.undoTarget;
  const preview = activity.undoPreview;
  // The native preview decides what goes back. The timeline may hold only
  // part of a large plan, so count from the preview and never drop the rest.
  const restoring =
    batch && preview
      ? batch.entries.filter((entry) => preview.entryIds.includes(entry.id))
      : [];
  const total = preview?.entryIds.length ?? 0;
  const unlisted = total - restoring.length;
  return (
    <Modal
      open={batch !== null}
      title={batch ? `Undo: ${batchTitle(batch)}` : "Undo"}
      onClose={activity.closeUndo}
      footer={
        <>
          <Button variant="ghost" onClick={activity.closeUndo}>
            {preview?.undoable ? "Cancel" : "Close"}
          </Button>
          {preview?.undoable && (
            <Button
              variant="primary"
              disabled={activity.undoBusy}
              onClick={activity.confirmUndo}
            >
              Undo {total === 1 ? "1 change" : `${total} changes`}
            </Button>
          )}
        </>
      }
    >
      {activity.undoBusy && !preview && (
        <p className="muted">Checking what can be undone…</p>
      )}
      {activity.undoFailure && (
        <RecoveryNotice
          error={activity.undoFailure}
          stage={activity.undoPartial ? "partialUndo" : "refused"}
          actions={{
            previewAgain: () => batch && activity.startUndo(batch),
          }}
        />
      )}
      {preview && preview.undoable && (
        <>
          <p>
            These files go back to how they were. Nothing changes until you
            confirm.
          </p>
          <ul className="activity-files">
            {restoring.map((entry) => (
              <li key={entry.id}>
                <ChangeLine
                  entry={{
                    ...entry,
                    beforeRelativePath: entry.afterRelativePath,
                    afterRelativePath: entry.beforeRelativePath,
                  }}
                />
              </li>
            ))}
          </ul>
          {unlisted > 0 && (
            <p className="muted">
              and {unlisted} more {unlisted === 1 ? "file" : "files"} from this
              change, not listed here
            </p>
          )}
        </>
      )}
      {preview && !preview.undoable && (
        <>
          <p>Folio won't undo this, so nothing will change:</p>
          <ul className="activity-files">
            {preview.conflicts.map((conflict) => (
              <li key={conflict.historyEntryId}>
                {conflict.relativePath} {CONFLICT_REASONS[conflict.reason]}.
              </li>
            ))}
          </ul>
        </>
      )}
    </Modal>
  );
}
