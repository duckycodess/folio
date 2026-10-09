import { Sparkles } from "lucide-react";
import { useEffect, useRef } from "react";
import type { CollectionsController } from "../app/useCollections";
import {
  keepProblem,
  MAX_COLLECTION_NAME_CHARS,
  nameOrigin,
  suggestionsNotice,
} from "../domain/collections";
import type { SuggestedCollection } from "../domain/contracts";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Notice } from "../ui/Notice";
import { Progress } from "../ui/Progress";

/**
 * Organize's suggested collections: files grouped by meaning by the local
 * embedding model, named by the local generation model. Keeping one stores
 * references only; no file is moved or copied, so there is no preview here.
 */
export function SuggestedCollections({
  collections,
  onStop = collections.stopSuggest,
}: {
  collections: CollectionsController;
  /** Stops grouping, and whatever local AI work follows it. */
  onStop?: () => void;
}) {
  const { suggestions } = collections;
  const stopped = useRef<HTMLParagraphElement>(null);
  // The Stop button is gone once stopped; focus goes to what happened instead.
  useEffect(() => {
    if (suggestions.status === "stopped") stopped.current?.focus();
  }, [suggestions.status]);
  if (suggestions.status === "idle") return null;

  return (
    <section
      className="suggested-collections"
      aria-labelledby="suggested-collections-heading"
    >
      <h3 id="suggested-collections-heading" className="subsection-title">
        Suggested collections
      </h3>
      <p className="muted">
        Collections are virtual: keeping one groups the files without moving or
        copying them.
      </p>

      {suggestions.status === "grouping" && (
        <div className="flow-step">
          <Progress label="Grouping files by meaning with the local model" />
          <div className="form-actions">
            <Button variant="secondary" onClick={onStop}>
              Stop grouping
            </Button>
          </div>
        </div>
      )}

      {suggestions.status === "stopped" && (
        <p ref={stopped} tabIndex={-1} className="muted">
          Grouping stopped. Analyze again to group files by meaning.
        </p>
      )}

      {suggestions.status === "failed" && suggestions.error && (
        <Notice
          tone="warning"
          action={
            <button
              type="button"
              className="link-button"
              onClick={() => void collections.suggest()}
            >
              Try again
            </button>
          }
        >
          Folio couldn't group these files: {suggestions.error.message}
        </Notice>
      )}

      {suggestions.status === "ready" && suggestions.result && (
        <>
          {(() => {
            const notice = suggestionsNotice(suggestions.result);
            return notice && <Notice tone={notice.tone}>{notice.text}</Notice>;
          })()}
          {suggestions.result.status === "grouped" &&
            (suggestions.result.groups.length ? (
              <ul className="collection-suggestions">
                {suggestions.result.groups.map((group, index) => (
                  <SuggestedGroup
                    key={group.id}
                    group={group}
                    number={index + 1}
                    collections={collections}
                  />
                ))}
              </ul>
            ) : (
              <p className="muted">
                No files are close enough in meaning to suggest a collection.
              </p>
            ))}
        </>
      )}
    </section>
  );
}

function SuggestedGroup({
  group,
  number,
  collections,
}: {
  group: SuggestedCollection;
  number: number;
  collections: CollectionsController;
}) {
  const { suggestions } = collections;
  const draft = suggestions.drafts[group.id];
  if (!draft) return null;
  const kept = suggestions.kept[group.id];
  const origin = nameOrigin(group, draft);
  const problem = keepProblem(group, draft);
  const keeping = suggestions.keeping === group.id;
  const refused =
    suggestions.keepError?.groupId === group.id
      ? suggestions.keepError.error
      : null;
  const nameId = `${group.id}-name`;
  const label = `Group ${number}`;

  return (
    <li className="collection-suggestion">
      <div className="collection-suggestion-name">
        <label htmlFor={nameId} className="field-label">
          {label}: collection name
        </label>
        <input
          id={nameId}
          className="text-input"
          value={draft.name}
          maxLength={MAX_COLLECTION_NAME_CHARS * 2}
          disabled={Boolean(kept)}
          placeholder={group.name ? undefined : "Type a name"}
          onChange={(event) =>
            collections.editName(group.id, event.target.value)
          }
        />
        {origin === "generated" && (
          <Badge>
            <Sparkles size={12} aria-hidden="true" /> Name written by local AI
          </Badge>
        )}
        {origin === "edited" && group.name && (
          <span className="muted">
            The local AI suggested “{group.name.text}”.
          </span>
        )}
      </div>

      {group.name && origin === "generated" && (
        <p className="muted collection-citations">
          Based on{" "}
          {group.name.citations
            .map(
              (citation) =>
                group.members.find(
                  (member) => member.documentId === citation.documentId,
                )?.relativePath ?? citation.documentId,
            )
            .join(", ")}
          .
        </p>
      )}

      <fieldset className="suggestions" disabled={Boolean(kept)}>
        <legend className="visually-hidden">{label}: files</legend>
        <ul className="suggestion-list">
          {group.members.map((member) => (
            <li key={member.documentId}>
              <label className="suggestion">
                <input
                  type="checkbox"
                  checked={draft.chosen.includes(member.documentId)}
                  onChange={() =>
                    collections.toggleMember(group.id, member.documentId)
                  }
                />
                <span className="suggestion-text">
                  <span className="plan-path">{member.relativePath}</span>
                  <span className="muted collection-passage">
                    “{excerpt(member.passage.text)}”
                  </span>
                </span>
              </label>
            </li>
          ))}
        </ul>
      </fieldset>

      {refused && <Notice tone="warning">{refused.message}</Notice>}

      {kept ? (
        <Notice tone="success">
          Kept as “{kept.name}”. No files were moved.
        </Notice>
      ) : (
        <div className="form-actions">
          <Button
            variant="secondary"
            disabled={Boolean(problem) || Boolean(suggestions.keeping)}
            onClick={() => collections.keep(group.id)}
          >
            {keeping ? "Keeping…" : "Keep collection"}
          </Button>
          {problem && <span className="muted">{problem}</span>}
        </div>
      )}
    </li>
  );
}

/** The first line of a passage, shortened for a list. */
function excerpt(text: string): string {
  const line = text.trim().split(/\r?\n/u).find(Boolean) ?? "";
  return line.length > 140 ? `${line.slice(0, 139)}…` : line;
}
