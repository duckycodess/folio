import { useEffect, useRef, useState } from "react";
import { folderChoices } from "../app/fileActions";
import type { Drafts } from "../app/drafts";
import { useAskAct } from "../app/useAskAct";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import type { DocumentRecord, OperationProposal } from "../domain/contracts";
import type { ViewId } from "../shell/navigation";
import { useAnnounce } from "../ui/Announcer";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Olio } from "../ui/Olio";
import { OlioSprite } from "../ui/OlioSprite";
import { Panel } from "../ui/Panel";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { TurnBody, type OpenFile } from "./AskTurns";
import { ChangeDialog } from "./ChangeDialog";

export type { OpenFile };

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
  // The running turn this page saw start; only its end is announced, so
  // stored history and switching conversations announce nothing.
  const watching = useRef<number | null>(null);
  const folders = folderChoices(workspace.documents).filter(Boolean);
  const root = workspace.workspace?.rootPath ?? "";
  const rootName = root.split(/[\\/]/).filter(Boolean).at(-1) ?? root;

  // Say when a request finishes; the result itself is on the page.
  useEffect(() => {
    if (!latest) return;
    if (latest.status === "running") {
      watching.current = latest.id;
      return;
    }
    if (watching.current !== latest.id) return;
    watching.current = null;
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
        <OlioSprite state={ask.busy ? "thinking" : "idle"} size={130} />
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
                    ? "keyword search for now (no search model, or it has not read this folder yet)"
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
