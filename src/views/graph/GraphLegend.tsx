import type { ConnectionKind } from "../../domain/connections";

interface LegendEntry {
  kind: ConnectionKind;
  label: string;
  description: string;
}

const CONFIRMED: LegendEntry[] = [
  {
    kind: "explicitReference",
    label: "Links",
    description:
      "A link written in one file to another. The arrow points to the linked file; arrows at both ends mean they link to each other.",
  },
  {
    kind: "exactDuplicate",
    label: "Identical copies",
    description: "Same contents, byte for byte.",
  },
];

const INFERRED: LegendEntry[] = [
  {
    kind: "similarity",
    label: "Similar content (AI)",
    description:
      "Found by the local AI model comparing passages. Check before relying on it.",
  },
  {
    kind: "sharedFactCandidate",
    label: "May state the same fact (AI)",
    description: "Suggested by the local AI model. Check before relying on it.",
  },
];

/** A short sample of the line each kind is drawn with. */
function Swatch({ kind }: { kind: ConnectionKind }) {
  return (
    <svg
      className={`graph-swatch graph-edge-${kind}`}
      width="40"
      height="14"
      viewBox="0 0 40 14"
      aria-hidden="true"
    >
      {kind === "exactDuplicate" ? (
        <g className="graph-edge is-confirmed">
          <line className="graph-edge-outer" x1="2" y1="7" x2="38" y2="7" />
          <line className="graph-edge-inner" x1="2" y1="7" x2="38" y2="7" />
        </g>
      ) : kind === "explicitReference" ? (
        <g className="graph-edge is-confirmed">
          <line x1="2" y1="7" x2="31" y2="7" />
          <path className="graph-arrow" d="M30,2.5 L38,7 L30,11.5 z" />
        </g>
      ) : (
        <line className="graph-edge is-inferred" x1="2" y1="7" x2="38" y2="7" />
      )}
    </svg>
  );
}

/**
 * What each line means, with a filter per kind. Model-found kinds are listed
 * only when a model actually produced some, so nothing suggests AI output
 * that doesn't exist.
 */
export function GraphLegend({
  counts,
  hidden,
  onToggle,
}: {
  counts: Partial<Record<ConnectionKind, number>>;
  hidden: ReadonlySet<ConnectionKind>;
  onToggle: (kind: ConnectionKind) => void;
}) {
  const inferred = INFERRED.filter((entry) => counts[entry.kind]);
  return (
    <fieldset className="graph-legend">
      <legend className="subsection-title">Show on the map</legend>
      <ul className="graph-legend-list">
        {[...CONFIRMED, ...inferred].map((entry) => (
          <li key={entry.kind} className="graph-legend-item">
            <label className="graph-legend-label">
              <input
                type="checkbox"
                checked={!hidden.has(entry.kind)}
                onChange={() => onToggle(entry.kind)}
              />
              <Swatch kind={entry.kind} />
              <span>
                {entry.label}{" "}
                <span className="muted tabular">
                  ({counts[entry.kind] ?? 0})
                </span>
              </span>
            </label>
            <p className="graph-legend-description">{entry.description}</p>
          </li>
        ))}
      </ul>
      {!inferred.length && (
        <p className="graph-legend-description">
          Connections found by the local AI model appear here when a model
          produces them.
        </p>
      )}
    </fieldset>
  );
}
