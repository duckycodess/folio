import { useAiIndexState } from "../app/useAiIndex";
import { coverageNotice } from "../domain/aiCoverage";

/**
 * Qualifies a short or empty list of AI connections: what Folio has and hasn't
 * compared yet. Nothing while the comparison is complete, or where there is no
 * coverage to report (the browser preview).
 */
export function AiCoverageNotice() {
  const ai = useAiIndexState();
  if (ai.refreshing) {
    return (
      <p className="muted" role="status">
        Checking AI connections…{" "}
        {ai.pairsCompleted > 0 && `${ai.pairsCompleted} pairs compared. `}
        <button type="button" className="link-button" onClick={ai.stop}>
          Stop
        </button>
      </p>
    );
  }
  const notice = coverageNotice(ai.coverage);
  if (!notice) return null;
  return (
    <p className="muted" role="status">
      {notice.message}{" "}
      {notice.canContinue && (
        <button type="button" className="link-button" onClick={ai.refresh}>
          Continue
        </button>
      )}
    </p>
  );
}
