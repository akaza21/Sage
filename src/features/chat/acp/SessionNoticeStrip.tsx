import { useState } from "react";

import type { SessionNotice } from "../../../domain/acpProjection";
import "./session-notice-strip.css";

const LEVEL_LABEL: Record<SessionNotice["level"], string> = {
  info: "Notice",
  warning: "Warning",
  error: "Error",
};

/** A notice's identity: the session surface already collapses repeats. */
const keyOf = (notice: SessionNotice) =>
  `${notice.level}\u0000${notice.source ?? ""}\u0000${notice.text}`;

/**
 * The current session's harness notices (warnings, configuration problems,
 * model reroutes), newest last. The list is already bounded by the session
 * surface; dismissing one hides it in this view only.
 */
export function SessionNoticeStrip({
  notices,
}: {
  notices: readonly SessionNotice[];
}) {
  const [dismissed, setDismissed] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const shown = notices.filter((notice) => !dismissed.has(keyOf(notice)));
  if (shown.length === 0) return null;
  const dismiss = (keys: readonly string[]) =>
    setDismissed((current) => new Set([...current, ...keys]));
  return (
    <section className="acp-session-notices" aria-label="Session notices">
      <ul>
        {shown.map((notice) => {
          const key = keyOf(notice);
          return (
            <li
              key={key}
              className={`acp-session-notice acp-session-notice--${notice.level}`}
            >
              <span className="acp-session-notice__level">
                {LEVEL_LABEL[notice.level]}
              </span>
              <span className="acp-session-notice__text">{notice.text}</span>
              {notice.source ? (
                <span className="acp-session-notice__source">
                  {notice.source}
                </span>
              ) : null}
              <button
                type="button"
                className="acp-session-notice__dismiss"
                aria-label={`Dismiss ${LEVEL_LABEL[notice.level].toLowerCase()}: ${notice.text}`}
                onClick={() => dismiss([key])}
              >
                ×
              </button>
            </li>
          );
        })}
      </ul>
      {shown.length > 1 ? (
        <button
          type="button"
          className="button--compact acp-session-notices__clear"
          onClick={() => dismiss(shown.map(keyOf))}
        >
          Dismiss all
        </button>
      ) : null}
    </section>
  );
}
