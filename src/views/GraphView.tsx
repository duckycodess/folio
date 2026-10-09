import { ArrowLeftRight, ArrowRight, Waypoints } from "lucide-react";
import { useEffect, useId, useState, type KeyboardEvent } from "react";
import { folderChoices } from "../app/fileActions";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import { describeConnection } from "../domain/connections";
import type { DocumentRecord } from "../domain/contracts";
import {
  folderSpread,
  graphPairs,
  isConfirmed,
  type GraphPair,
  type GraphStart,
} from "../domain/graphScope";
import { Badge } from "../ui/Badge";
import { EmptyState } from "../ui/EmptyState";
import { FileTypeIcon } from "../ui/FileTypeIcon";
import { Panel } from "../ui/Panel";
import {
  ConnectionEvidence,
  CoverageNote,
  originalLocation,
} from "./Connections";

type StartKind = GraphStart["kind"];

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
}: {
  pairs: GraphPair[];
  workspace: WorkspaceState;
  relations: RelationshipsState;
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
                onOpen={() => workspace.selectDocument(first)}
              />
              {directed ? (
                <ArrowRight size={16} aria-label="links to" />
              ) : (
                <ArrowLeftRight size={16} aria-label="and" />
              )}
              <FileEnd
                document={second}
                workspace={workspace}
                onOpen={() => workspace.selectDocument(second)}
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
      return start.term.trim()
        ? `Connections about “${start.term.trim()}”`
        : "Connections about a topic";
  }
}

/**
 * Relationships as a keyboard- and screen-reader-friendly list, starting from
 * every file, one file, a folder or a topic. Confirmed connections (links,
 * identical bytes) are kept apart from suggestions. A drawn graph would be an
 * extra view, never the only one.
 */
export function GraphView({
  workspace,
  relations,
}: {
  workspace: WorkspaceState;
  relations: RelationshipsState;
}) {
  const { request } = relations;
  useEffect(request, [request]);
  const ids = useId();
  const documents = [...workspace.documents].sort((a, b) =>
    a.relativePath.localeCompare(b.relativePath),
  );
  const folders = folderChoices(workspace.documents).filter(Boolean);
  const byId = new Map(documents.map((d) => [d.id, d]));

  const [kind, setKind] = useState<StartKind>(() =>
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

  const start: GraphStart =
    kind === "file"
      ? {
          kind,
          documentId: byId.has(fileId) ? fileId : (documents[0]?.id ?? ""),
        }
      : kind === "folder"
        ? {
            kind,
            folder: folders.includes(folder) ? folder : (folders[0] ?? ""),
          }
        : kind === "topic"
          ? { kind, term }
          : { kind };
  const pairs = graphPairs(documents, relations.connectionsOf, start);
  const confirmed = pairs.filter((pair) => isConfirmed(pair.connection));
  const suggested = pairs.filter((pair) => !isConfirmed(pair.connection));
  const spread = folderSpread(pairs);
  const root = workspace.workspace?.rootPath ?? "Sample files";

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
                read.
              </p>
            </div>
          )}
        </div>
      </Panel>

      <Panel
        title={scopeTitle(start, byId)}
        actions={
          <>
            {workspace.source === "samples" && <Badge>Sample files</Badge>}
            <Badge>{pairs.length} found</Badge>
          </>
        }
      >
        <CoverageNote relations={relations} />
        {pairs.length ? (
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
                />
              </section>
            )}
            {suggested.length > 0 && (
              <section aria-labelledby={`${ids}-suggested`}>
                <h3 id={`${ids}-suggested`} className="graph-list-heading">
                  Suggested <Badge>{suggested.length}</Badge>
                </h3>
                <p className="muted">
                  Found by comparing passages. Check the evidence before relying
                  on them.
                </p>
                <PairList
                  pairs={suggested}
                  workspace={workspace}
                  relations={relations}
                />
              </section>
            )}
          </div>
        ) : (
          <EmptyState
            icon={<Waypoints size={24} />}
            title={
              start.kind === "topic" && !start.term.trim()
                ? "Type a topic to start"
                : "No connections found"
            }
          >
            Folio connects files through links written inside them and identical
            copies.
          </EmptyState>
        )}
      </Panel>

      {pairs.length > 0 && (
        <Panel title="Where these files are">
          <ul className="folder-spread">
            {spread.map(({ folder: name, files }) => (
              <li key={name}>
                <span className="related-path">
                  {name ? `${root}/${name}` : root}
                </span>
                <Badge>
                  {files} {files === 1 ? "file" : "files"}
                </Badge>
              </li>
            ))}
          </ul>
          <p className="muted">
            A written relationship summary needs a local AI model, which isn't
            available in this version yet. The connections and evidence above
            don't need one.
          </p>
        </Panel>
      )}
    </div>
  );
}
