import {
  recoveryFor,
  type RecoveryActionKind,
  type RecoveryStage,
} from "../app/recovery";
import type { FolioError } from "../domain/errors";
import { Button } from "./Button";
import { Notice } from "./Notice";

interface RecoveryNoticeProps {
  error: Pick<FolioError, "code"> & Partial<Pick<FolioError, "details">>;
  /** For change-related errors: refused before any write (the default), during apply, or a partial Undo. */
  stage?: RecoveryStage;
  /** The next steps this screen can carry out; others aren't offered. */
  actions?: Partial<Record<RecoveryActionKind, () => void>>;
  onDismiss?: () => void;
}

/** What happened and what to do next, for any failure. */
export function RecoveryNotice({
  error,
  stage = "refused",
  actions = {},
  onDismiss,
}: RecoveryNoticeProps) {
  const recovery = recoveryFor(error, stage);
  let action = recovery.action;
  // A screen that can't stop other work still offers its plain retry.
  if (action?.kind === "stopAndRetry" && !actions.stopAndRetry && actions.retry)
    action = { kind: "retry", label: "Try again" };
  const run = action && actions[action.kind];
  return (
    <Notice
      tone={recovery.tone}
      action={
        (run || onDismiss) && (
          <div className="notice-actions">
            {run && action && (
              <Button variant="secondary" onClick={run}>
                {action.label}
              </Button>
            )}
            {onDismiss && (
              <button type="button" className="link-button" onClick={onDismiss}>
                Dismiss
              </button>
            )}
          </div>
        )
      }
    >
      <p className="notice-title">{recovery.title}</p>
      <p>{recovery.message}</p>
    </Notice>
  );
}
