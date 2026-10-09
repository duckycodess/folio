import { useMemo } from "react";
import type { WorkspaceState } from "../app/useWorkspace";
import { Badge } from "../ui/Badge";
import { EmptyState } from "../ui/EmptyState";
import { Panel } from "../ui/Panel";
import { FileList } from "./FileList";
import { WorkspaceSource } from "./WorkspaceSource";

export function FilesView({ workspace }: { workspace: WorkspaceState }) {
  const searching = workspace.query.trim().length > 0;
  const documents = useMemo(
    () =>
      (searching
        ? workspace.results.map((result) => result.document)
        : workspace.documents
      )
        .slice()
        .sort((a, b) => a.relativePath.localeCompare(b.relativePath)),
    [searching, workspace.results, workspace.documents],
  );

  return (
    <div className="view">
      <header className="page-header page-header-compact">
        <h1 className="page-title">Files</h1>
      </header>
      <WorkspaceSource workspace={workspace} />
      <Panel
        title={searching ? "Search results" : "All files"}
        actions={
          <Badge>
            {documents.length} {documents.length === 1 ? "file" : "files"}
          </Badge>
        }
      >
        {documents.length ? (
          <FileList
            label={searching ? "Search results" : "All files"}
            documents={documents}
            selectedId={workspace.selected?.id}
            onSelect={workspace.selectDocument}
          />
        ) : (
          <EmptyState title={searching ? "No matching files" : "No files yet"}>
            {searching
              ? "Try another word."
              : "Folio reads text, Markdown and text-based PDF files."}
          </EmptyState>
        )}
      </Panel>
    </div>
  );
}
