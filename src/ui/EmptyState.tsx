import type { ReactNode } from "react";

interface EmptyStateProps {
  icon?: ReactNode;
  /** An `<Olio>` pose; shown instead of the icon. */
  illustration?: ReactNode;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
  /** One line, for sections that must not push content down. */
  compact?: boolean;
}

export function EmptyState({
  icon,
  illustration,
  title,
  children,
  action,
  compact = false,
}: EmptyStateProps) {
  return (
    <div className={`empty-state${compact ? " empty-state-compact" : ""}`}>
      {illustration ? (
        <span className="empty-state-illustration">{illustration}</span>
      ) : (
        icon && (
          <span className="empty-state-icon" aria-hidden="true">
            {icon}
          </span>
        )
      )}
      <p className="empty-state-title">{title}</p>
      {children && <div className="empty-state-body">{children}</div>}
      {action && <div className="empty-state-action">{action}</div>}
    </div>
  );
}
