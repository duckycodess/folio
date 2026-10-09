import {
  describeProposal,
  matchReason,
  methodLabel,
  type AskTurn,
} from "../app/askAct";
import { requestForFile } from "../app/proposals";
import type { AskActController } from "../app/useAskAct";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import type {
  DocumentRecord,
  OperationProposal,
  SearchResult,
  SourcePassage,
} from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { Notice } from "../ui/Notice";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { CitedSentences } from "./CitedSentences";

export type OpenFile = (document: DocumentRecord, tab?: "Summary") => void;

const EXCERPTS_SHOWN = 2;

function flat(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

export function ResultList({
  results,
  query,
  onOpen,
  onOpenPassage,
  action,
}: {
  results: SearchResult[];
  query: string;
  onOpen: OpenFile;
  onOpenPassage: (passage: SourcePassage) => void;
  /** An extra button per file, such as "Summarize". */
  action?: {
    label: (document: DocumentRecord) => string;
    run: (document: DocumentRecord) => void;
  };
}) {
  return (
    <ul className="ask-results">
      {results.map((result) => (
        <li key={result.document.id} className="ask-result">
          <div className="ask-result-head">
            <FileTypeIcon mediaType={result.document.mediaType} size={20} />
            <span className="related-text">
              <span className="related-name">{result.document.name}</span>
              <span
                className="related-path"
                title={result.document.relativePath}
              >
                {result.document.relativePath}
              </span>
            </span>
            <Badge>{methodLabel(result.method)}</Badge>
          </div>
          <p className="muted">{matchReason(result, query)}</p>
          {result.passages.length > 0 && (
            <ul className="evidence-list">
              {result.passages.slice(0, EXCERPTS_SHOWN).map((passage) => (
                <li key={`${passage.start}-${passage.end}`}>
                  <button
                    type="button"
                    className="evidence-item"
                    onClick={() => onOpenPassage(passage)}
                    aria-label={`Show passage in ${result.document.name}: ${flat(passage.text)}`}
                  >
                    <span className="evidence-text">{flat(passage.text)}</span>
                    {passage.page !== undefined && (
                      <span className="evidence-where">
                        page {passage.page}
                      </span>
                    )}
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="ask-result-actions">
            {action && (
              <Button
                variant="primary"
                onClick={() => action.run(result.document)}
              >
                {action.label(result.document)}
              </Button>
            )}
            <Button onClick={() => onOpen(result.document)}>Open file</Button>
          </div>
        </li>
      ))}
    </ul>
  );
}

export function TurnBody({
  turn,
  ask,
  workspace,
  relations,
  onOpen,
  onRetry,
  onNavigate,
  onPreviewChange,
}: {
  turn: AskTurn;
  ask: AskActController;
  workspace: WorkspaceState;
  relations: RelationshipsState;
  onOpen: OpenFile;
  onRetry: () => void;
  onNavigate: (view: ViewId) => void;
  onPreviewChange: (proposal: OperationProposal) => void;
}) {
  if (turn.status === "running")
    return (
      <div className="ask-running">
        {turn.outcome?.type === "practice" ? (
          <div className="ask-practice" aria-live="off">
            <Badge>Practice replies — not a model</Badge>
            <p>
              {turn.outcome.reply}
              <span className="ask-practice-cursor" aria-hidden="true" />
            </p>
          </div>
        ) : (
          <Progress
            label={
              turn.action === "find"
                ? "Looking through your files"
                : "Olio is working on your request"
            }
          />
        )}
        {turn.action === "ask" && <Button onClick={ask.cancel}>Cancel</Button>}
      </div>
    );
  if (turn.status === "cancelled")
    return <p className="muted">Cancelled. Nothing was changed.</p>;
  if (turn.status === "failed" && turn.error)
    return (
      <RecoveryNotice
        error={turn.error}
        actions={{ retry: onRetry, openModelLab: () => onNavigate("modelLab") }}
      />
    );

  const outcome = turn.outcome;
  if (!outcome) return null;
  const byId = new Map(workspace.documents.map((d) => [d.id, d]));
  switch (outcome.type) {
    case "results":
      return outcome.results.length ? (
        <>
          {outcome.namesOnly && (
            <Notice tone="info">
              Searched file names only: the local search model isn't set up yet,
              so the files' text wasn't searched.
            </Notice>
          )}
          <p className="muted">
            {outcome.results.length}{" "}
            {outcome.results.length === 1 ? "file" : "files"} found. Excerpts
            are quoted from the files.
          </p>
          <ResultList
            results={outcome.results}
            query={outcome.query}
            onOpen={onOpen}
            onOpenPassage={relations.openPassage}
          />
        </>
      ) : (
        <p>No files {ask.scope ? `in ${ask.scope} ` : ""}match this request.</p>
      );
    case "answer":
      return outcome.result.kind === "insufficientEvidence" ? (
        <Notice tone="info">
          Olio couldn't find enough in your files to answer this. Nothing was
          made up.
        </Notice>
      ) : (
        <div className="summary">
          <div className="summary-head">
            <Badge>Generated answer</Badge>
            <Badge>Not reviewed</Badge>
          </div>
          <CitedSentences
            result={outcome.result}
            byId={byId}
            onOpen={relations.openPassage}
          />
          <p className="muted">
            Made by the local model {outcome.result.modelId}. Each point links
            to the passage it came from.
          </p>
        </div>
      );
    case "summary":
      return (
        <div className="ask-summary">
          <p>
            Summarizing <strong>{outcome.document.name}</strong>. The summary
            appears in that file's Summary tab.
          </p>
          <Button
            variant="primary"
            onClick={() => onOpen(outcome.document, "Summary")}
          >
            Open Summary tab
          </Button>
        </div>
      );
    case "chooseFile":
      if (!outcome.candidates.length)
        return <p>Olio couldn't find a file that matches. Try naming it.</p>;
      return (
        <>
          <p>
            {outcome.purpose === "summarize"
              ? "Which file should Olio summarize?"
              : "This could mean several files."}
          </p>
          {outcome.purpose === "change" && (
            <p className="muted">
              Choose the file, and Olio will read the request again for it.
              Nothing changes until you approve the exact preview.
            </p>
          )}
          <ResultList
            results={outcome.candidates}
            query={turn.request}
            onOpen={onOpen}
            onOpenPassage={relations.openPassage}
            action={
              outcome.purpose === "summarize"
                ? {
                    label: (document) => `Summarize ${document.name}`,
                    run: (document) => ask.chooseForSummary(turn.id, document),
                  }
                : {
                    label: (document) => `Use ${document.name}`,
                    run: (document) =>
                      ask.ask(
                        requestForFile(turn.request, document.relativePath),
                        document,
                      ),
                  }
            }
          />
        </>
      );
    case "clarify":
      return (
        <Notice tone="info">Olio needs more detail: {outcome.question}</Notice>
      );
    case "proposal": {
      const target =
        "documentId" in outcome.proposal
          ? byId.get(outcome.proposal.documentId)
          : undefined;
      return (
        <>
          <p>
            Olio understood:{" "}
            <strong>{describeProposal(outcome.proposal)}</strong>
          </p>
          <p className="muted">
            Nothing changes until you approve the exact preview.
          </p>
          <div className="ask-result-actions">
            <Button
              variant="primary"
              disabled={ask.busy}
              onClick={() => onPreviewChange(outcome.proposal)}
            >
              Preview change…
            </Button>
            {target && (
              <Button onClick={() => onOpen(target)}>Open {target.name}</Button>
            )}
          </div>
        </>
      );
    }
    case "unsupported":
      return (
        <Notice tone="info">
          Olio can't do that here: {outcome.reason} Nothing was changed.
        </Notice>
      );
    case "otherFile":
      return (
        <Notice tone="warning">
          Olio proposed a change to another file than the one you chose (
          {outcome.chosen.relativePath}): {describeProposal(outcome.proposal)}.
          Nothing was changed. Try naming the change more precisely.
        </Notice>
      );
    case "unreadable":
      return (
        <Notice tone="warning">
          Olio's reply couldn't be understood, so nothing was done. Try
          rephrasing your request.
        </Notice>
      );
    case "practice":
      return (
        <div className="ask-practice">
          <Badge>Practice replies — not a model</Badge>
          <p>{outcome.reply}</p>
        </div>
      );
  }
}
