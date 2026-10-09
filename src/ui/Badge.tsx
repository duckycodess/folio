import type { ReactNode } from "react";

interface BadgeProps {
  children: ReactNode;
  /** Decorative dot color; the text label always carries the meaning. */
  dot?: string;
}

export function Badge({ children, dot }: BadgeProps) {
  return (
    <span className="badge">
      {dot && (
        <span
          className="badge-dot"
          style={{ background: dot }}
          aria-hidden="true"
        />
      )}
      {children}
    </span>
  );
}
