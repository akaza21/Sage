import { useState } from "react";

import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import {
  continuedNoticeBefore,
  refusedNoticeText,
  refusedSessionBinding,
  sessionContinuationDraft,
} from "../../domain/sessionBinding";

/** Shown once, above the first Turn after the session crossed a drift. */
export function SessionDriftNotice({
  page,
  turn,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
}) {
  const text = continuedNoticeBefore(page, turn);
  if (!text) return null;
  return (
    <div className="session-binding-notice" role="note">
      {text}
    </div>
  );
}

/**
 * Where the composer would resume an ended session: explains why the
 * installed build cannot continue it and offers a new session that starts
 * from a reviewable summary of the visible conversation.
 */
export function SessionBindingRecovery({
  page,
  onContinue,
}: {
  page: AcpChatPage;
  onContinue: (draft: string) => void;
}) {
  const [continuedChat, setContinuedChat] = useState<string | null>(null);
  const refusal = refusedSessionBinding(page);
  if (!refusal) return null;
  const continued = continuedChat === page.chatId;
  return (
    <section
      className="session-binding-recovery"
      role="status"
      aria-label="Session can't be resumed"
    >
      <p className="session-binding-recovery__headline">
        This session can't be resumed.
      </p>
      <p>{refusedNoticeText(refusal)}</p>
      <div className="session-binding-recovery__actions">
        <button
          className="button button--compact"
          type="button"
          disabled={continued}
          onClick={() => {
            onContinue(sessionContinuationDraft(page.title, page));
            setContinuedChat(page.chatId);
          }}
        >
          Continue in a new session
        </button>
        <span>
          {continued
            ? "A summary of this chat is in your message. Review it, then send to start the new session."
            : "The agent starts fresh. Sage adds a summary of this chat that you can review before sending."}
        </span>
      </div>
    </section>
  );
}
