import { useState } from "react";
import type { CollectionsController } from "../app/useCollections";
import type { WorkspaceState } from "../app/useWorkspace";
import { membersLabel, nameProblem } from "../domain/collections";
import type { VirtualCollection } from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Notice } from "../ui/Notice";
import { Olio } from "../ui/Olio";
import { Panel } from "../ui/Panel";

/**
 * Kept collections. Renaming or removing a collection, or taking a file out
 * of one, changes no file, so none of it goes through a plan.
 */
export function CollectionsPanel({
  workspace,
  collections,
  onAnalyze,
}: {
  workspace: WorkspaceState;
  collections: CollectionsController;
  /** Makes the collection Organize's target. */
  onAnalyze: (collectionId: string) => void;
}) {
  if (!collections.available || !collections.collections.length)
    return (
      <Panel title="Collections">
        <EmptyState
          illustration={<Olio pose="organizing" size={96} />}
          title="No collections yet"
        >
          Collections are virtual: they group related files without moving or
          copying them.{" "}
          {collections.available
            ? "Analyze this folder to get suggestions you can keep."
            : "They work on your own folder in the desktop app."}
        </EmptyState>
      </Panel>
    );

  return (
    <Panel title="Collections">
      <p className="muted">
        Collections group files without moving or copying them. Removing one
        leaves its files where they are.
      </p>
      {collections.error && (
        <Notice
          tone="warning"
          action={
            <button
              type="button"
              className="link-button"
              onClick={collections.dismissError}
            >
              Dismiss
            </button>
          }
        >
          {collections.error.message}
        </Notice>
      )}
      <ul className="collection-list">
        {collections.collections.map((collection) => (
          <CollectionItem
            key={collection.id}
            collection={collection}
            workspace={workspace}
            collections={collections}
            onAnalyze={onAnalyze}
          />
        ))}
      </ul>
    </Panel>
  );
}

function CollectionItem({
  collection,
  workspace,
  collections,
  onAnalyze,
}: {
  collection: VirtualCollection;
  workspace: WorkspaceState;
  collections: CollectionsController;
  onAnalyze: (collectionId: string) => void;
}) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const headingId = `${collection.id}-heading`;
  const problem = renaming === null ? null : nameProblem(renaming);

  return (
    <li className="collection-item" aria-labelledby={headingId}>
      <div className="collection-item-header">
        <h3 id={headingId} className="subsection-title">
          {collection.name}
        </h3>
        <span className="muted">{membersLabel(collection)}</span>
      </div>

      {renaming !== null ? (
        <form
          className="collection-rename"
          onSubmit={(event) => {
            event.preventDefault();
            if (problem) return;
            void collections
              .rename(collection.id, renaming)
              .then((done) => done && setRenaming(null));
          }}
        >
          <label htmlFor={`${collection.id}-rename`} className="field-label">
            New name for “{collection.name}”
          </label>
          <input
            id={`${collection.id}-rename`}
            className="text-input"
            value={renaming}
            aria-invalid={problem ? true : undefined}
            onChange={(event) => setRenaming(event.target.value)}
            autoFocus
          />
          {problem && <span className="muted">{problem}</span>}
          <div className="form-actions">
            <Button
              type="submit"
              variant="secondary"
              disabled={Boolean(problem)}
            >
              Save name
            </Button>
            <Button variant="ghost" onClick={() => setRenaming(null)}>
              Cancel
            </Button>
          </div>
        </form>
      ) : (
        <div className="form-actions">
          <Button variant="secondary" onClick={() => onAnalyze(collection.id)}>
            Analyze this collection
          </Button>
          <Button variant="ghost" onClick={() => setRenaming(collection.name)}>
            Rename
          </Button>
          {confirmRemove ? (
            <>
              <Button
                variant="secondary"
                onClick={() => collections.remove(collection.id)}
              >
                Remove “{collection.name}”
              </Button>
              <Button variant="ghost" onClick={() => setConfirmRemove(false)}>
                Keep it
              </Button>
            </>
          ) : (
            <Button variant="ghost" onClick={() => setConfirmRemove(true)}>
              Remove collection
            </Button>
          )}
        </div>
      )}

      <ul className="collection-members">
        {collection.members.map((member) => {
          const document = workspace.documents.find(
            (item) => item.id === member.documentId,
          );
          return (
            <li key={member.documentId} className="collection-member">
              {document && !member.missing ? (
                <button
                  type="button"
                  className="link-button plan-path"
                  onClick={() => void workspace.selectDocument(document)}
                >
                  {member.relativePath}
                </button>
              ) : (
                <span className="plan-path">{member.relativePath}</span>
              )}
              {member.missing && (
                <Badge>Missing: moved or deleted outside Folio</Badge>
              )}
              <button
                type="button"
                className="link-button"
                aria-label={`Remove ${member.relativePath} from ${collection.name}`}
                onClick={() =>
                  collections.removeMember(collection.id, member.documentId)
                }
              >
                Remove from collection
              </button>
            </li>
          );
        })}
      </ul>
    </li>
  );
}
