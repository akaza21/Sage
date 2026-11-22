import { useState, type ReactNode } from "react";
import { CircleAlert, Info, TriangleAlert } from "./leanIcons";
import "./notice.css";

export type NoticeSeverity = "info" | "warning" | "blocking";

export interface NoticeProps {
  /** Problem kind and owning session, turn, or resource; never message text. */
  identity: string;
  severity: NoticeSeverity;
  message: ReactNode;
  action?: { label: string; onClick: () => void; disabled?: boolean };
  details?: ReactNode;
  progress?: boolean;
  /** Historical failures are readable without replaying a live announcement. */
  announce?: boolean;
}

export function Notice(props: NoticeProps) {
  return <NoticeContent key={props.identity} {...props} />;
}

function NoticeContent({
  identity,
  severity,
  message,
  action,
  details,
  progress,
  announce = true,
}: NoticeProps) {
  const [expanded, setExpanded] = useState(false);
  const error = severity === "blocking";
  const warning = severity === "warning";
  const blocking = error && announce;
  // Undefined is unannounced; false permanently quiets this identity.
  const [announcement, setAnnouncement] = useState<
    { message: ReactNode } | false
  >();
  // Settle announcement state before committing changed copy to the tree.
  // Identity changes remount this bounded state; StrictMode replays preserve it.
  if (blocking && announcement === undefined) setAnnouncement({ message });
  else if (announcement && announcement.message !== message)
    setAnnouncement(false);
  const Icon = error ? CircleAlert : warning ? TriangleAlert : Info;
  return (
    <div
      className={`notice notice--${severity}`}
      aria-live={
        progress ? "polite" : blocking && announcement ? "assertive" : "off"
      }
      data-notice-identity={identity}
      role={blocking ? "alert" : progress ? "status" : undefined}
    >
      <Icon size={14} />
      <div className="notice__body">
        <span className="sr-only">
          {error ? "Error: " : warning ? "Warning: " : ""}
        </span>
        <span>{message}</span>
        {action ? (
          <button
            type="button"
            className="notice__action"
            onClick={action.onClick}
            disabled={action.disabled}
          >
            {action.label}
          </button>
        ) : null}
        {details ? (
          <>
            <button
              type="button"
              className="notice__text-link"
              aria-expanded={expanded}
              onClick={() => setExpanded(!expanded)}
            >
              Details
            </button>
            {expanded ? (
              <div className="notice__details" aria-live="off">
                {details}
              </div>
            ) : null}
          </>
        ) : null}
      </div>
    </div>
  );
}
