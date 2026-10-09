import { useEffect } from "react";
import { useAnnounce } from "./Announcer";

interface ProgressProps {
  label: string;
  /** 0–100. Omit when the real progress is unknown. */
  value?: number;
}

/** Never invent a percentage: unknown progress renders as indeterminate. */
export function Progress({ label, value }: ProgressProps) {
  const known = value !== undefined;
  const announce = useAnnounce();
  // Say once that work started; the result is announced by whatever follows.
  useEffect(() => announce(`${label}…`), [announce, label]);
  return (
    <div className="progress">
      <div className="progress-label">
        <span>{label}</span>
        {known && <span className="tabular">{Math.round(value)}%</span>}
      </div>
      <div
        className={`progress-track${known ? "" : " progress-indeterminate"}`}
        role="progressbar"
        aria-label={label}
        aria-valuemin={known ? 0 : undefined}
        aria-valuemax={known ? 100 : undefined}
        aria-valuenow={known ? Math.round(value) : undefined}
      >
        <div
          className="progress-fill"
          style={known ? { width: `${value}%` } : undefined}
        />
      </div>
    </div>
  );
}
