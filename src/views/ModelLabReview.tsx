import { useId, useState, type FormEvent } from "react";
import {
  latestReview,
  recordModelLabel,
  REVIEW_LABELS,
  reviewMaterial,
} from "../app/modelLab";
import type { ModelLabController, ReviewInput } from "../app/useModelLab";
import type { BenchmarkRecord } from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { Notice } from "../ui/Notice";

const REVIEWER_KEY = "folio.modelLab.reviewer";

/** The last reviewer's name on this device; storage may be unavailable. */
function rememberedReviewer(): string {
  try {
    return window.localStorage.getItem(REVIEWER_KEY) ?? "";
  } catch {
    return "";
  }
}

function rememberReviewer(name: string) {
  try {
    window.localStorage.setItem(REVIEWER_KEY, name);
  } catch {
    // Not remembering the name only means typing it again next time.
  }
}

const VERDICTS: [ReviewInput["status"], string][] = [
  ["correct", "Correct"],
  ["partiallyCorrect", "Partly correct"],
  ["incorrect", "Incorrect"],
];

/**
 * A person's review of one recorded summary. It is appended to the record and
 * bound to the exact output shown here; the measured result is never changed.
 */
export function ReviewDialog({
  record,
  lab,
  onClose,
}: {
  record: BenchmarkRecord;
  lab: ModelLabController;
  onClose: () => void;
}) {
  const formId = useId();
  const material = reviewMaterial(record);
  const previous = latestReview(record);
  const [status, setStatus] = useState<ReviewInput["status"] | null>(null);
  const [reviewer, setReviewer] = useState(rememberedReviewer);
  const [notes, setNotes] = useState("");
  const [saving, setSaving] = useState(false);
  const [failure, setFailure] = useState<FolioError | null>(null);
  const ready = status !== null && reviewer.trim() !== "" && !saving;

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!ready || !status) return;
    setSaving(true);
    setFailure(null);
    lab
      .saveReview(record, {
        status,
        reviewer: reviewer.trim(),
        notes: notes.trim() || undefined,
      })
      .then(() => {
        rememberReviewer(reviewer.trim());
        onClose();
      })
      .catch((cause: unknown) => {
        // The choices stay as they were, so the review isn't lost.
        setFailure(toFolioError(cause));
        setSaving(false);
      });
  }

  return (
    <Modal
      open
      title={`Review summary: ${record.caseId}`}
      className="modal-wide"
      onClose={onClose}
      dismissible={!saving}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={saving}>
            Cancel
          </Button>
          <Button
            type="submit"
            form={formId}
            variant="primary"
            disabled={!ready}
          >
            {saving ? "Saving…" : "Save review"}
          </Button>
        </>
      }
    >
      <p className="muted">
        {recordModelLabel(record)}. Your review is added to this result and
        names the exact output below. The measurement itself isn't changed.
      </p>
      {previous && (
        <p className="muted">
          Last review: {REVIEW_LABELS[previous.status]} by {previous.reviewer}.
        </p>
      )}
      {failure && (
        // The native core's message names what to do for a review, unlike
        // the general recovery copy for its code (a changed search passage).
        <Notice tone="danger">
          <p>The review wasn't saved.</p>
          <p>
            {failure.code === "evidenceInvalid"
              ? "The recorded output changed since it was shown, so Folio read it again. Check it below before saving."
              : failure.message}{" "}
            Your choices are kept.
          </p>
        </Notice>
      )}
      <dl className="lab-conditions">
        {material.request && (
          <div>
            <dt>Request</dt>
            <dd>{material.request}</dd>
          </div>
        )}
        {material.document && (
          <div>
            <dt>File</dt>
            <dd>{material.document}</dd>
          </div>
        )}
      </dl>
      <h3 className="graph-list-heading">Summary written</h3>
      {material.summary ? (
        <blockquote className="lab-output">{material.summary}</blockquote>
      ) : (
        <p className="muted">The recorded output has no summary text.</p>
      )}
      {material.passages.length > 0 && (
        <details className="lab-passages">
          <summary>
            Passages the model was given ({material.passages.length})
          </summary>
          <ol>
            {material.passages.map((passage, index) => (
              <li key={index}>{passage}</li>
            ))}
          </ol>
        </details>
      )}
      <form id={formId} className="lab-review-form" onSubmit={submit}>
        <fieldset className="lab-choice" disabled={saving}>
          <legend className="lab-legend">Is the summary correct?</legend>
          {VERDICTS.map(([verdict, label]) => (
            <label key={verdict} className="lab-model">
              <input
                type="radio"
                name={`${formId}-verdict`}
                checked={status === verdict}
                onChange={() => setStatus(verdict)}
              />
              <span>{label}</span>
            </label>
          ))}
        </fieldset>
        <label className="field-label" htmlFor={`${formId}-reviewer`}>
          Your name
        </label>
        <input
          id={`${formId}-reviewer`}
          className="text-input"
          value={reviewer}
          disabled={saving}
          autoComplete="name"
          onChange={(event) => setReviewer(event.target.value)}
        />
        <label className="field-label" htmlFor={`${formId}-notes`}>
          Notes (optional)
        </label>
        <textarea
          id={`${formId}-notes`}
          className="text-area"
          value={notes}
          disabled={saving}
          rows={3}
          onChange={(event) => setNotes(event.target.value)}
        />
      </form>
    </Modal>
  );
}
