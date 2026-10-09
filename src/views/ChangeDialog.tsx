import { useEffect, useRef, useState } from "react";
import { preparePassageEdit } from "../adapters/actions";
import { readNativeDocument } from "../adapters/workspace";
import { describeProposal } from "../app/askAct";
import { changedRegion, proposalOperation } from "../app/proposals";
import { useOrganize } from "../app/useOrganize";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import type {
  ActionPlan,
  OperationProposal,
  SourcePassage,
} from "../domain/contracts";
import { folioError } from "../domain/errors";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { PreviewStep, ResultStep } from "./OrganizeFlowPanel";

function flat(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/** The exact text change in the one file being edited. */
function EditDiff({ before, after }: { before: string; after: string }) {
  const region = changedRegion(before, after);
  if (!region) return null;
  return (
    <figure className="edit-diff">
      <figcaption className="field-label">
        Text change, from line {region.line}
      </figcaption>
      <pre className="source-text">
        {region.clippedStart && "…"}
        {region.before}
        {region.removed && (
          <del aria-label={`Removed: ${region.removed}`}>{region.removed}</del>
        )}
        {region.added && (
          <ins aria-label={`Added: ${region.added}`}>{region.added}</ins>
        )}
        {region.after}
        {region.clippedEnd && "…"}
      </pre>
    </figure>
  );
}

/** Ripple: related passages to check afterwards. They are never changed. */
function RippleList({
  plan,
  onOpen,
}: {
  plan: ActionPlan;
  onOpen: (passage: SourcePassage) => void;
}) {
  if (!plan.impacts.length) return null;
  return (
    <section className="ripple" aria-labelledby="ripple-title">
      <h4 id="ripple-title" className="field-label">
        Related passages to review afterwards
      </h4>
      <p className="muted">
        Folio won't change these files. Check them once the change is saved.
      </p>
      <ul className="ripple-list">
        {plan.impacts.map((impact) => (
          <li key={impact.documentId}>
            <div className="ripple-head">
              <span className="related-name">{impact.relativePath}</span>
              <Badge>Needs review</Badge>
              {impact.strength === "similarityOnly" && (
                <Badge>Similar wording only</Badge>
              )}
            </div>
            <p className="muted">{impact.reason}</p>
            {impact.evidence.map((passage) => (
              <button
                key={`${passage.start}-${passage.end}`}
                type="button"
                className="evidence-item"
                onClick={() => onOpen(passage)}
                aria-label={`Show passage in ${impact.relativePath}: ${flat(passage.text)}`}
              >
                <span className="evidence-text">{flat(passage.text)}</span>
              </button>
            ))}
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * A change Olio understood, made only through the native plan: the exact
 * preview (diff and Ripple), Approve and Save, then the result with Undo.
 * Nothing in the request or in retrieved text can approve it.
 */
export function ChangeDialog({
  proposal,
  workspace,
  relations,
  onClose,
}: {
  proposal: OperationProposal;
  workspace: WorkspaceState;
  relations: RelationshipsState;
  onClose: () => void;
}) {
  const organize = useOrganize(workspace, "assistant", relations.refresh);
  const { state } = organize;
  const heading = useRef<HTMLHeadingElement>(null);
  const [before, setBefore] = useState<string | null>(null);
  const started = useRef(false);

  function prepare() {
    started.current = true;
    setBefore(null);
    organize.previewFrom(async (folder) => {
      const simple = proposalOperation(proposal);
      if (simple) return [simple];
      if (proposal.kind !== "edit") return [];
      const operation = await preparePassageEdit(
        folder,
        proposal.documentId,
        proposal.find,
        proposal.replace,
      );
      // The edit must apply to the revision Olio read, not a newer one.
      if (
        operation.kind !== "edit" ||
        operation.expectedContentHash !== proposal.observedContentHash
      )
        throw folioError(
          "targetChanged",
          "The file changed after Olio read it.",
        );
      const document = workspace.documents.find(
        (item) => item.id === proposal.documentId,
      );
      if (!document)
        throw folioError("documentUnavailable", "File not listed.");
      const read =
        document.content !== undefined &&
        document.contentHash === operation.expectedContentHash
          ? document
          : await readNativeDocument(folder, document);
      // The diff must be drawn against the revision the edit applies to.
      if (read.contentHash !== operation.expectedContentHash)
        throw folioError(
          "targetChanged",
          "The file changed after Olio read it.",
        );
      setBefore(read.content ?? null);
      return [operation];
    });
  }

  // The preview is prepared as soon as the dialog opens. If effects run
  // again (React's development checks reset the flow), the newest request
  // wins, so this runs each time too.
  useEffect(() => {
    prepare();
  }, []);

  useEffect(() => {
    if (state.stage === "preview" || state.stage === "result")
      heading.current?.focus();
  }, [state.stage]);

  const edit = state.plan?.operations.find((item) => item.kind === "edit");

  return (
    <Modal
      open
      title={describeProposal(proposal)}
      dismissible={state.stage !== "applying"}
      onClose={() => {
        organize.done();
        onClose();
      }}
    >
      {state.stage === "result" ? (
        <ResultStep
          organize={organize}
          heading={heading}
          onDone={() => {
            organize.done();
            onClose();
          }}
        />
      ) : state.stage === "preview" || state.stage === "applying" ? (
        <PreviewStep
          organize={organize}
          heading={heading}
          cancelLabel="Cancel"
          details={
            <>
              {edit?.kind === "edit" && before !== null && (
                <EditDiff before={before} after={edit.after} />
              )}
              {state.plan && (
                <RippleList plan={state.plan} onOpen={relations.openPassage} />
              )}
            </>
          }
        />
      ) : state.stage === "preparing" ? (
        <Progress label="Preparing the exact preview" />
      ) : state.error ? (
        <RecoveryNotice
          error={state.error}
          actions={{ retry: prepare, previewAgain: prepare }}
          onDismiss={() => {
            organize.done();
            onClose();
          }}
        />
      ) : started.current ? (
        <div className="flow-step">
          <p>Nothing was changed.</p>
          <div className="form-actions ask-actions">
            <Button onClick={prepare}>Preview again</Button>
            <Button
              variant="primary"
              onClick={() => {
                organize.done();
                onClose();
              }}
            >
              Close
            </Button>
          </div>
        </div>
      ) : (
        <Progress label="Preparing the exact preview" />
      )}
    </Modal>
  );
}
