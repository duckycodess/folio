import { SEARCH_OPERATORS } from "../domain/searchQuery";

/**
 * The search operators Find files understands. Choosing an example adds it
 * to the request, so it can be edited before searching.
 */
export function SearchCheatsheet({
  id,
  onUse,
}: {
  id: string;
  onUse: (example: string) => void;
}) {
  return (
    <section
      id={id}
      className="search-cheatsheet"
      aria-label="Search cheatsheet"
    >
      <p className="search-cheatsheet-lead">
        Use these with <strong>Find files</strong>. Choose one to add it to your
        request.
      </p>
      <dl className="search-cheatsheet-list">
        {SEARCH_OPERATORS.map((operator) => (
          <div key={operator.example} className="search-cheatsheet-row">
            <dt>
              <button
                type="button"
                className="search-cheatsheet-example"
                onClick={() => onUse(operator.example)}
              >
                <code>{operator.example}</code>
              </button>
            </dt>
            <dd>{operator.meaning}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
