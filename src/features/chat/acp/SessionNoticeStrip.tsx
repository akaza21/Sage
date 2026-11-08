import { useState } from "react";

import type { AcpChatPage, SessionNotice } from "../../../domain/acpProjection";
import "./session-notice-strip.css";
import { SessionSurfacePanel } from "./SessionSurfacePanel";

const LEVEL_LABEL: Record<SessionNotice["level"], string> = {
  info: "Notice",
  warning: "Warning",
  error: "Error",
};

/** A notice's identity: the session surface already collapses repeats. */
const keyOf = (notice: SessionNotice) =>
  `${notice.level}\u0000${notice.source ?? ""}\u0000${notice.text}`;

/** The page's session notices, then its background activities and Undo. */
export function SessionStrip({ page }: { page: AcpChatPage }) {
  return (
    <>
      {page.sessionNotices?.length ? (
        <SessionNoticeStrip
          session={page.chatId + page.currentSessionId}
          notices={page.sessionNotices}
        />
      ) : null}
      {page.sessionSurface ? (
        <SessionSurfacePanel
          surface={page.sessionSurface}
          chatId={page.chatId}
        />
      ) : null}
    </>
  );
}

interface Dismissed {
  session: string;
  keys: ReadonlySet<string>;
}

/**
 * The current session's harness notices (warnings, configuration problems,
 * model reroutes), newest last. The list is already bounded by the session
 * surface; dismissing one hides it in this view only. Dismissals belong to
 * one `session` and to the notices it still has, so another session starts
 * clean and the dismissed set never outgrows the list.
 */
export function SessionNoticeStrip({
  session,
  notices,
}: {
  /** Identifies the session the notices belong to. */
  session: string;
  notices: readonly SessionNotice[];
}) {
  const [dismissed, setDismissed] = useState<Dismissed>(() => ({
    session,
    keys: new Set(),
  }));
  const current = new Set(notices.map(keyOf));
  const kept =
    dismissed.session === session
      ? [...dismissed.keys].filter((key) => current.has(key))
      : [];
  if (dismissed.session !== session || kept.length !== dismissed.keys.size) {
    setDismissed({ session, keys: new Set(kept) });
  }
  const shown = notices.filter((notice) => !kept.includes(keyOf(notice)));
  if (shown.length === 0) return null;
  const dismiss = (keys: readonly string[]) =>
    setDismissed({ session, keys: new Set([...kept, ...keys]) });
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
