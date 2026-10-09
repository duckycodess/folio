import { CircleAlert, CircleCheck, Info } from "lucide-react";
import type { ReactNode } from "react";

export type NoticeTone = "info" | "success" | "warning" | "danger";

interface NoticeProps {
  tone?: NoticeTone;
  children: ReactNode;
  action?: ReactNode;
}

const ICONS = {
  info: Info,
  success: CircleCheck,
  warning: CircleAlert,
  danger: CircleAlert,
} as const;

/** Errors interrupt (`alert`); everything else is announced politely. */
export function Notice({ tone = "info", children, action }: NoticeProps) {
  const Icon = ICONS[tone];
  return (
    <div
      className={`notice notice-${tone}`}
      role={tone === "danger" ? "alert" : "status"}
    >
      <Icon className="notice-icon" size={18} aria-hidden="true" />
      <div className="notice-body">{children}</div>
      {action && <div className="notice-action">{action}</div>}
    </div>
  );
}
