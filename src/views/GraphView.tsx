import {
  ArrowLeftRight,
  ArrowRight,
  List,
  Network,
  Waypoints,
} from "lucide-react";
import {
  useDeferredValue,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { summarizeRelationships } from "../adapters/ai";
import { folderChoices } from "../app/fileActions";
import { isGenerationReady } from "../app/models";
import type { RelationshipsState } from "../app/useRelationships";
import { useModels } from "../app/useModels";
import { useAiIndexState } from "../app/useAiIndex";
import { mayClaimNoConnections, summaryBasisLine } from "../domain/aiCoverage";
import { AiCoverageNotice } from "../ui/AiCoverageNotice";
import type { WorkspaceState } from "../app/useWorkspace";
import { describeConnection } from "../domain/connections";
import { hasSearchWords } from "../domain/discovery";
import type { DocumentRecord, GroundedResult } from "../domain/contracts";
import { toFolioError, type FolioError } from "../domain/errors";
import {
  folderSpread,
  graphPairs,
  isConfirmed,
  relationshipSummaryScope,
  type GraphPair,
  type GraphStart,
} from "../domain/graphScope";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { Panel } from "../ui/Panel";
import {
  ConnectionEvidence,
  CoverageNote,
  folderLocation,
  originalLocation,
} from "./Connections";
import { ConceptMap } from "./graph/ConceptMap";
import { CitedSentences } from "./CitedSentences";
import { Notice } from "../ui/Notice";
import { RecoveryNotice } from "../ui/RecoveryNotice";

type StartKind = GraphStart["kind"];
type GraphMode = "map" | "list";

/** The last Map/List choice, kept while the app runs. */
let rememberedMode: GraphMode = "map";

const START_LABELS: Record<StartKind, string> = {
  all: "All files",
  file: "A file",
  folder: "A folder",
  topic: "A topic",
};

function FileEnd({
  document,
  workspace,
  onOpen,
}: {
  document: DocumentRecord;
  workspace: WorkspaceState;
  onOpen: () => void;
}) {
  return (
    <button type="button" className="connection-end" onClick={onOpen}>
      <FileTypeIcon mediaType={document.mediaType} size={20} />
      <span className="related-text">
        <span className="related-name">{document.name}</span>
        <span className="related-path" title={document.relativePath}>
          {originalLocation(document, workspace)}
        </span>
      </span>
    </button>
  );
}

function PairList({
  pairs,
  workspace,
  relations,
  onOpen,
}: {
  pairs: GraphPair[];
  workspace: WorkspaceState;
  relations: RelationshipsState;
  onOpen: (document: DocumentRecord) => void;
}) {
  const byId = new Map(workspace.documents.map((d) => [d.id, d]));
  return (
    <ul className="relationship-list">
      {pairs.map(({ from, to, connection }) => {
        const label = describeConnection(connection);
        const directed =
          connection.direction === "outgoing" ||
          connection.direction === "incoming";
        const [first, second] =
          connection.direction === "incoming" ? [to, from] : [from, to];
        return (
          <li
            key={`${connection.kind}-${from.id}-${to.id}`}
            className="connection connection-pair"
          >
            <div className="connection-ends">
              <FileEnd
                document={first}
                workspace={workspace}
                onOpen={() => onOpen(first)}
              />
              {directed ? (
                <ArrowRight size={16} aria-label="links to" />
              ) : (
                <ArrowLeftRight size={16} aria-label="and" />
              )}
              <FileEnd
                document={second}
                workspace={workspace}
                onOpen={() => onOpen(second)}
              />
            </div>
            <p className="connection-provenance">
              <Badge>
                {connection.kind === "explicitReference" ? "Link" : label.type}
              </Badge>{" "}
              {label.provenance}
            </p>
            <ConnectionEvidence
              evidence={connection.evidence}
              byId={byId}
              onOpen={relations.openPassage}
            />
          </li>
        );
      })}
    </ul>
  );
}

/** Arrow keys, Home and End move between the files in the list. */
function moveBetweenFiles(event: KeyboardEvent<HTMLDivElement>) {
  const target = event.target as HTMLElement;
  if (!target.classList.contains("connection-end")) return;
  const ends = [
    ...event.currentTarget.querySelectorAll<HTMLElement>(".connection-end"),
  ];
  const index = ends.indexOf(target);
  const next =
    event.key === "ArrowDown" || event.key === "ArrowRight"
      ? index + 1
      : event.key === "ArrowUp" || event.key === "ArrowLeft"
        ? index - 1
        : event.key === "Home"
          ? 0
          : event.key === "End"
            ? ends.length - 1
            : null;
  if (next === null) return;
  event.preventDefault();
  ends[Math.max(0, Math.min(ends.length - 1, next))]?.focus();
}

function scopeTitle(start: GraphStart, byId: Map<string, DocumentRecord>) {
  switch (start.kind) {
    case "all":
      return "Connections between files";
    case "file":
      return `Connected to ${byId.get(start.documentId)?.name ?? "this file"}`;
    case "folder":
      return `Connections in ${start.folder}`;
    case "topic":
      return hasSearchWords(start.term)
        ? `Connections about “${start.term.trim()}”`
        : "Connections about a topic";
  }
}

/**
 * Relationships starting from every file, one file, a folder or a topic, as a
 * concept map or as a keyboard- and screen-reader-friendly list. Both are
 * drawn from the same pairs; the list is an equal alternative, never a
 * fallback. In the list, confirmed connections (links, identical bytes) are
 * kept apart from suggestions.
 */
export function GraphView({
  workspace,
  relations,
}: {
  workspace: WorkspaceState;
  relations: RelationshipsState;
}) {
  const { request } = relations;
  const models = useModels();
  const aiIndex = useAiIndexState();
  const generationReady =
    models.load === "ready" &&
    isGenerationReady(models.groups, models.setup, models.runtime);
  useEffect(request, [request]);
  const ids = useId();
  const documents = useMemo(
    () =>
      [...workspace.documents].sort((a, b) =>
        a.relativePath.localeCompare(b.relativePath),
      ),
    [workspace.documents],
  );
  const folders = folderChoices(workspace.documents).filter(Boolean);
  const byId = new Map(documents.map((d) => [d.id, d]));

  const [chosenKind, setKind] = useState<StartKind>(() =>
    workspace.selected ? "file" : "all",
  );
  const [fileId, setFileId] = useState(
    () => workspace.selected?.id ?? documents[0]?.id ?? "",
  );
  // Opening a file anywhere, including from this list, makes it the file to
  // start from, so the user can walk from one file to the next.
  const selectedId = workspace.selected?.id;
  useEffect(() => {
    if (selectedId) setFileId(selectedId);
  }, [selectedId]);
  const [folder, setFolder] = useState(() => folders[0] ?? "");
  const [term, setTerm] = useState("");
  // Typing stays responsive while the list catches up with the term.
  const deferredTerm = useDeferredValue(term);

  // A folder with no subfolders has nothing to choose, so show everything.
  const kind = chosenKind === "folder" && !folders.length ? "all" : chosenKind;
  const documentId = byId.has(fileId) ? fileId : (documents[0]?.id ?? "");
  const folderName = folders.includes(folder) ? folder : (folders[0] ?? "");
  const start: GraphStart = useMemo(
    () =>
      kind === "file"
        ? { kind, documentId }
        : kind === "folder"
          ? { kind, folder: folderName }
          : kind === "topic"
            ? { kind, term: deferredTerm }
            : { kind },
    [kind, documentId, folderName, deferredTerm],
  );
  const { connectionsOf } = relations;
  const pairs = useMemo(
    () => graphPairs(documents, connectionsOf, start),
    [documents, connectionsOf, start],
  );
  const confirmed = pairs.filter((pair) => isConfirmed(pair.connection));
  const suggested = pairs.filter((pair) => !isConfirmed(pair.connection));
  const spread = folderSpread(pairs);
  const summaryScope = useMemo(
    () =>
      relationshipSummaryScope(
        pairs,
        start.kind === "file" ? start.documentId : undefined,
      ),
    [pairs, start],
  );
  const summaryDocumentIds = summaryScope.documentIds;
  const summaryScopeKey = summaryDocumentIds.join("|");
  const [summary, setSummary] = useState<GroundedResult | null>(null);
  const [summaryBusy, setSummaryBusy] = useState(false);
  const [summaryError, setSummaryError] = useState<FolioError | null>(null);
  const summaryRequest = useRef(0);
  useEffect(() => {
    summaryRequest.current += 1;
    setSummary(null);
    setSummaryError(null);
    setSummaryBusy(false);
    // A summary describes the connections and coverage it was written from;
    // when either changes (a refresh, a new model), it is dropped.
  }, [
    summaryScopeKey,
    summaryScope.totalDocuments,
    start.kind === "file" ? start.documentId : "",
    aiIndex.coverage?.state,
    aiIndex.coverage?.pairsConsidered,
    aiIndex.coverage?.spaceFingerprint,
    relations.relationships,
  ]);
  async function writeRelationshipSummary() {
    const folderId = workspace.workspace?.id;
    if (!folderId || summaryDocumentIds.length === 0) return;
    const requestId = ++summaryRequest.current;
    setSummaryBusy(true);
    setSummaryError(null);
    try {
      const result = await summarizeRelationships(
        folderId,
        summaryDocumentIds,
        start.kind === "file" ? start.documentId : undefined,
      );
      if (requestId === summaryRequest.current) setSummary(result);
    } catch (cause) {
      if (requestId === summaryRequest.current)
        setSummaryError(toFolioError(cause));
    } finally {
      if (requestId === summaryRequest.current) setSummaryBusy(false);
    }
  }
  const [mode, setMode] = useState<GraphMode>(rememberedMode);
  function choose(next: GraphMode) {
    rememberedMode = next;
    setMode(next);
  }

  // Opening a file from the list in "A file" mode rebuilds the list, which
  // removes the button that had focus. Move focus to the new title so the
  // keyboard user stays in the list and hears where they are.
  const listTitle = useRef<HTMLHeadingElement>(null);
  const walking = useRef(false);
  function openFile(document: DocumentRecord) {
    walking.current = kind === "file" && document.id !== documentId;
    void workspace.selectDocument(document);
  }
  useEffect(() => {
    if (!walking.current) return;
    walking.current = false;
    const active = window.document.activeElement;
    if (!active || active === window.document.body || !active.isConnected)
      listTitle.current?.focus();
  }, [documentId]);

  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Graph</h1>
        <p className="page-tagline">
          How your files connect, with the evidence.
        </p>
      </header>

      <Panel title="Start from">
        <div className="graph-start">
          <fieldset className="choice-group">
            <legend className="visually-hidden">Start from</legend>
            {(Object.keys(START_LABELS) as StartKind[]).map((option) => (
              <label key={option} className="choice">
                <input
                  type="radio"
                  name={`${ids}-start`}
                  value={option}
                  checked={kind === option}
                  disabled={option === "folder" && !folders.length}
                  onChange={() => setKind(option)}
                />
                {START_LABELS[option]}
              </label>
            ))}
          </fieldset>
          {kind === "file" && (
            <div className="graph-start-field">
              <label htmlFor={`${ids}-file`} className="field-label">
                File
              </label>
              <select
                id={`${ids}-file`}
                className="text-input"
                value={start.kind === "file" ? start.documentId : ""}
                onChange={(event) => setFileId(event.target.value)}
              >
                {documents.map((document) => (
                  <option key={document.id} value={document.id}>
                    {document.relativePath}
                  </option>
                ))}
              </select>
            </div>
          )}
          {kind === "folder" && (
            <div className="graph-start-field">
              <label htmlFor={`${ids}-folder`} className="field-label">
                Folder
              </label>
              <select
                id={`${ids}-folder`}
                className="text-input"
                value={start.kind === "folder" ? start.folder : ""}
                onChange={(event) => setFolder(event.target.value)}
              >
                {folders.map((choice) => (
                  <option key={choice} value={choice}>
                    {choice}
                  </option>
                ))}
              </select>
            </div>
          )}
          {kind === "topic" && (
            <div className="graph-start-field">
              <label htmlFor={`${ids}-topic`} className="field-label">
                Topic or search term
              </label>
              <input
                id={`${ids}-topic`}
                type="search"
                className="text-input"
                value={term}
                aria-describedby={`${ids}-topic-help`}
                onChange={(event) => setTerm(event.target.value)}
              />
              <p id={`${ids}-topic-help`} className="field-help">
                Keyword match on file names and the text of files Folio has
                read. A file matches if it has any of the words.
              </p>
            </div>
          )}
        </div>
      </Panel>

      <Panel
        title={scopeTitle(start, byId)}
        titleRef={listTitle}
        actions={
          <>
            {workspace.source === "samples" && <Badge>Sample files</Badge>}
            <Badge>{pairs.length} found</Badge>
          </>
        }
      >
        <CoverageNote relations={relations} />
        {pairs.length ? (
          <>
            <div className="segmented" role="group" aria-label="Show as">
              <button
                type="button"
                className="segmented-option"
                aria-pressed={mode === "map"}
                onClick={() => choose("map")}
              >
                <Network size={16} aria-hidden="true" />
                Map
              </button>
              <button
                type="button"
                className="segmented-option"
                aria-pressed={mode === "list"}
                onClick={() => choose("list")}
              >
                <List size={16} aria-hidden="true" />
                List
              </button>
            </div>
            {mode === "map" ? (
              <ConceptMap
                workspace={workspace}
                relations={relations}
                pairs={pairs}
              />
            ) : (
              // Arrow keys move between files across both lists.
              <div className="graph-lists" onKeyDown={moveBetweenFiles}>
                {confirmed.length > 0 && (
                  <section aria-labelledby={`${ids}-confirmed`}>
                    <h3 id={`${ids}-confirmed`} className="graph-list-heading">
                      Confirmed <Badge>{confirmed.length}</Badge>
                    </h3>
                    <p className="muted">
                      Links written in the files and identical copies.
                    </p>
                    <PairList
                      pairs={confirmed}
                      workspace={workspace}
                      relations={relations}
                      onOpen={openFile}
                    />
                  </section>
                )}
                {suggested.length > 0 && (
                  <section aria-labelledby={`${ids}-suggested`}>
                    <h3 id={`${ids}-suggested`} className="graph-list-heading">
                      Suggested <Badge>{suggested.length}</Badge>
                    </h3>
                    <p className="muted">
                      Similar passages and possible shared facts. Check the
                      evidence before relying on them.
                    </p>
                    <PairList
                      pairs={suggested}
                      workspace={workspace}
                      relations={relations}
                      onOpen={openFile}
                    />
                  </section>
                )}
              </div>
            )}
          </>
        ) : (
          <EmptyState
            icon={<Waypoints size={24} />}
            title={
              start.kind === "topic" && !hasSearchWords(start.term)
                ? "Type a topic to start"
                : mayClaimNoConnections(aiIndex.coverage) || !aiIndex.coverage
                  ? "No connections found"
                  : "No connections found so far"
            }
          >
            Folio connects files through links written inside them and identical
            copies.
            <AiCoverageNotice />
          </EmptyState>
        )}
      </Panel>

      {pairs.length > 0 && (
        <Panel title="Where these files are">
          <ul className="folder-spread">
            {spread.map(({ folder: name, files }) => (
              <li key={name}>
                <span className="related-path">
                  {folderLocation(name, workspace)}
                </span>
                <Badge>
                  {files} {files === 1 ? "file" : "files"}
                </Badge>
              </li>
            ))}
          </ul>
          <div className="form-actions">
            <Button
              variant="primary"
              disabled={summaryBusy || !generationReady}
              onClick={() => void writeRelationshipSummary()}
            >
              {summaryBusy
                ? "Writing relationship summary…"
                : "Write relationship summary"}
            </Button>
          </div>
          {!generationReady && (
            <p className="muted">
              Set up and select an installed writing model and runtime in Model
              Lab to write a relationship summary. The connections and evidence
              above do not need a model.
            </p>
          )}
          {summaryScope.totalDocuments > summaryDocumentIds.length && (
            <p className="muted">
              This preview is based on {summaryDocumentIds.length} of{" "}
              {summaryScope.totalDocuments} connected files, prioritizing the
              selected file and its strongest neighbours.
            </p>
          )}
          {summaryError && (
            <RecoveryNotice
              error={summaryError}
              actions={{ retry: () => void writeRelationshipSummary() }}
            />
          )}
          {summary && summary.kind === "insufficientEvidence" && (
            <Notice tone="info">
              There is not enough relationship evidence for a summary.
            </Notice>
          )}
          {summary && summary.kind !== "insufficientEvidence" && (
            <div className="summary">
              <div className="summary-head">
                <Badge>Relationship summary</Badge>
                <Badge>Generated preview, not saved</Badge>
              </div>
              <p className="muted">
                Made by the local model {summary.modelId} (revision{" "}
                {summary.revision.slice(0, 12)}). Not reviewed for accuracy:
                each point links to its evidence.
              </p>
              {summary.basis && (
                <p className="muted">{summaryBasisLine(summary.basis)}</p>
              )}
              <CitedSentences result={summary} onOpen={relations.openPassage} />
            </div>
          )}
        </Panel>
      )}
    </div>
  );
}
