import { useEffect, useRef, useState } from "react";
import { folderChoices } from "../app/fileActions";
import {
  describeProposal,
  matchReason,
  methodLabel,
  type AskTurn,
} from "../app/askAct";
import type { Drafts } from "../app/drafts";
import { requestForFile } from "../app/proposals";
import { useAskAct, type AskActController } from "../app/useAskAct";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import type {
  DocumentRecord,
  OperationProposal,
  SearchResult,
  SourcePassage,
} from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { useAnnounce } from "../ui/Announcer";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { Notice } from "../ui/Notice";
import { Olio } from "../ui/Olio";
import { Panel } from "../ui/Panel";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { ChangeDialog } from "./ChangeDialog";
import { CitedSentences } from "./CitedSentences";

export type OpenFile = (document: DocumentRecord, tab?: "Summary") => void;

const EXCERPTS_SHOWN = 2;

function flat(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function ResultList({
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

function TurnBody({
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
        <Progress
          label={
            turn.action === "find"
              ? "Looking through your files"
              : "Olio is working on your request"
          }
        />
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
  }
}

/**
 * Ask & Act: a full-page workspace for asking Olio about the open folder.
 * Finding files needs no writing model. Answers and summaries cite their
 * sources. A change goes only through the native preview and approval.
 */
export function AssistantView({
  workspace,
  relations,
  drafts,
  onNavigate,
  onOpenFile,
}: {
  workspace: WorkspaceState;
  relations: RelationshipsState;
  drafts: Drafts;
  onNavigate: (view: ViewId) => void;
  onOpenFile: OpenFile;
}) {
  const ask = useAskAct(workspace);
  const [changing, setChanging] = useState<OperationProposal | null>(null);
  // The dialog unmounts when it closes, so focus goes back to its button here.
  const changeOpener = useRef<Element | null>(null);
  function previewChange(proposal: OperationProposal) {
    changeOpener.current = document.activeElement;
    setChanging(proposal);
  }
  function closeChange() {
    setChanging(null);
    const opener = changeOpener.current;
    if (opener instanceof HTMLElement)
      requestAnimationFrame(() => opener.isConnected && opener.focus());
  }
  const announce = useAnnounce();
  const request = drafts.instruction;
  const latest = ask.turns.at(-1);
  const announced = useRef<number | null>(null);
  const folders = folderChoices(workspace.documents).filter(Boolean);
  const root = workspace.workspace?.rootPath ?? "";
  const rootName = root.split(/[\\/]/).filter(Boolean).at(-1) ?? root;

  // Say when a request finishes; the result itself is on the page.
  useEffect(() => {
    if (
      !latest ||
      latest.status === "running" ||
      announced.current === latest.id
    )
      return;
    announced.current = latest.id;
    const outcome = latest.outcome;
    announce(
      latest.status === "cancelled"
        ? "Request cancelled."
        : latest.status === "failed"
          ? "The request didn't finish."
          : outcome?.type === "results"
            ? `${outcome.results.length} files found.`
            : "Olio's reply is ready.",
    );
  }, [latest, announce]);

  if (!ask.desktop || !ask.folderId)
    return (
      <div className="view">
        <header className="page-header page-header-compact">
          <h1 className="page-title">Ask &amp; Act</h1>
        </header>
        <Panel title="Ask Olio about your files">
          <EmptyState
            illustration={<Olio pose="peeking" size={160} />}
            title={
              ask.desktop
                ? "Add a folder to ask about"
                : "Ask & Act needs the desktop app"
            }
            action={
              ask.desktop ? (
                <Button variant="primary" onClick={() => onNavigate("home")}>
                  Go to Home
                </Button>
              ) : (
                <Button onClick={() => onNavigate("modelLab")}>
                  Open Model Lab
                </Button>
              )
            }
          >
            {ask.desktop
              ? "Olio searches a folder you've added. Sample files can be browsed and searched on Home."
              : "Olio runs on local AI models in the Folio desktop app. Searching and reading files work on Home."}
          </EmptyState>
        </Panel>
      </div>
    );

  const index = ask.index;
  const send = (action: "find" | "ask") =>
    action === "find" ? ask.find(request) : ask.ask(request);
  const turns = [...ask.turns].reverse();

  return (
    <div className="view">
      <header className="page-header page-header-compact ask-header">
        <Olio pose="thinking" size={96} />
        <div>
          <h1 className="page-title">Ask &amp; Act</h1>
          <p className="page-tagline">
            Ask Olio to find, explain or summarize your files. Nothing changes
            without your approval.
          </p>
        </div>
      </header>

      <section className="ask-scope" aria-label="Search scope">
        <label htmlFor="ask-scope" className="field-label">
          Searching in
        </label>
        <select
          id="ask-scope"
          className="text-input"
          value={ask.scope}
          disabled={ask.busy}
          onChange={(event) => ask.setScope(event.target.value)}
        >
          <option value="">All of {rootName}</option>
          {folders.map((folder) => (
            <option key={folder} value={folder}>
              {rootName}/{folder}
            </option>
          ))}
        </select>
        <p className="muted ask-index">
          {ask.preparing
            ? "Preparing this folder for search…"
            : index
              ? `${index.documentCount} files prepared for ${
                  index.method === "keyword"
                    ? "keyword search only (no search model)"
                    : "search by meaning"
                }.${
                  index.skippedDocuments?.length
                    ? ` ${index.skippedDocuments.length} skipped (unsupported or unreadable).`
                    : ""
                }`
              : "Not prepared yet. Folio prepares the folder on your first request."}
          {ask.scope && " Questions look across the whole folder."}
        </p>
        <Button disabled={ask.preparing || ask.busy} onClick={ask.prepare}>
          {index ? "Prepare again" : "Prepare now"}
        </Button>
        {ask.indexError && (
          <RecoveryNotice
            error={ask.indexError}
            actions={{
              retry: ask.prepare,
              openModelLab: () => onNavigate("modelLab"),
            }}
          />
        )}
        {index?.skippedDocuments && index.skippedDocuments.length > 0 && (
          <details className="ask-skipped">
            <summary>Skipped files</summary>
            <ul>
              {index.skippedDocuments.map((item) => (
                <li key={item.relativePath}>
                  {item.relativePath}: {item.reason}
                </li>
              ))}
            </ul>
          </details>
        )}
      </section>

      <Panel title="Your request">
        <form
          className="assistant-form"
          onSubmit={(event) => {
            event.preventDefault();
            send("ask");
          }}
        >
          <label htmlFor="instruction" className="visually-hidden">
            Your request
          </label>
          <textarea
            id="instruction"
            className="text-area"
            rows={3}
            value={request}
            placeholder="Find my notes about interview methods"
            aria-describedby="instruction-help"
            onChange={(event) => drafts.setInstruction(event.target.value)}
          />
          <p id="instruction-help" className="field-help">
            English, Filipino or Taglish. Find files works without a writing
            model.
          </p>
          <div className="form-actions ask-actions">
            <Button
              disabled={!request.trim() || ask.busy}
              onClick={() => send("find")}
            >
              Find files
            </Button>
            <Button
              type="submit"
              variant="primary"
              disabled={!request.trim() || ask.busy}
            >
              Ask Olio
            </Button>
          </div>
        </form>
      </Panel>

      {turns.length > 0 && (
        <section className="ask-turns" aria-label="Olio's replies">
          {turns.map((turn) => (
            <article key={turn.id} className="ask-turn">
              <header className="ask-turn-head">
                <Badge>
                  {turn.action === "find" ? "Find files" : "Ask Olio"}
                </Badge>
                <q className="ask-request">{turn.request}</q>
              </header>
              <TurnBody
                turn={turn}
                ask={ask}
                workspace={workspace}
                relations={relations}
                onOpen={onOpenFile}
                onRetry={() =>
                  turn.action === "find"
                    ? ask.find(turn.request)
                    : ask.ask(turn.request, turn.chosen)
                }
                onNavigate={onNavigate}
                onPreviewChange={previewChange}
              />
            </article>
          ))}
          {!ask.busy && (
            <Button variant="ghost" onClick={ask.clear}>
              Clear replies
            </Button>
          )}
        </section>
      )}
      {changing && (
        <ChangeDialog
          proposal={changing}
          workspace={workspace}
          relations={relations}
          onClose={closeChange}
        />
      )}
    </div>
  );
}
