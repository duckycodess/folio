import { useState } from "react";
import type { CollectionsController } from "../app/useCollections";
import { membersLabel } from "../domain/collections";
import type { DocumentRecord } from "../domain/contracts";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { Notice } from "../ui/Notice";

/**
 * Adds one file to a kept collection. Collections hold references only, so
 * nothing is previewed or approved: the file stays where it is.
 */
export function AddToCollectionDialog({
  document,
  collections,
  onClose,
}: {
  document: DocumentRecord;
  collections: CollectionsController;
  onClose: () => void;
}) {
  const holds = (id: string) =>
    collections.collections
      .find((collection) => collection.id === id)
      ?.members.some((member) => member.documentId === document.id) ?? false;
  const [chosen, setChosen] = useState(
    () =>
      collections.collections.find((collection) => !holds(collection.id))?.id ??
      "",
  );
  const [busy, setBusy] = useState(false);
  const [added, setAdded] = useState<string | null>(null);
  const name = collections.collections.find(
    (collection) => collection.id === chosen,
  )?.name;

  async function add() {
    if (!chosen || holds(chosen)) return;
    setBusy(true);
    const done = await collections.addMember(chosen, document.id);
    setBusy(false);
    if (done) setAdded(name ?? "");
  }

  return (
    <Modal
      open
      title={`Add ${document.name} to a collection`}
      onClose={onClose}
      footer={
        added === null ? (
          <>
            <Button
              variant="primary"
              disabled={!chosen || holds(chosen) || busy}
              onClick={() => void add()}
            >
              {busy ? "Adding…" : "Add to collection"}
            </Button>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
          </>
        ) : (
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        )
      }
    >
      {added !== null ? (
        <Notice tone="success">
          Added to “{added}”. The file stays where it is.
        </Notice>
      ) : (
        <fieldset className="suggestions">
          <legend className="field-label">Collection</legend>
          <ul className="suggestion-list">
            {collections.collections.map((collection) => (
              <li key={collection.id}>
                <label className="suggestion">
                  <input
                    type="radio"
                    name="add-to-collection"
                    value={collection.id}
                    checked={chosen === collection.id}
                    disabled={holds(collection.id)}
                    onChange={() => setChosen(collection.id)}
                  />
                  <span className="suggestion-text">
                    <span>{collection.name}</span>
                    <span className="muted">
                      {holds(collection.id)
                        ? "Already in this collection"
                        : membersLabel(collection)}
                    </span>
                  </span>
                </label>
              </li>
            ))}
          </ul>
        </fieldset>
      )}
      {collections.error && added === null && (
        <Notice tone="warning">{collections.error.message}</Notice>
      )}
    </Modal>
  );
}
