import type { ReactNode } from "react";

interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
  /** One line, for sections that must not push content down. */
  compact?: boolean;
}

export function EmptyState({
  icon,
  title,
  children,
  action,
  compact = false,
}: EmptyStateProps) {
  return (
    <div className={`empty-state${compact ? " empty-state-compact" : ""}`}>
      {icon && (
        <span className="empty-state-icon" aria-hidden="true">
          {icon}
        </span>
      )}
      <p className="empty-state-title">{title}</p>
      {children && <div className="empty-state-body">{children}</div>}
      {action && <div className="empty-state-action">{action}</div>}
    </div>
  );
}
