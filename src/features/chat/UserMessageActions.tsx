import { useContext } from "react";
import { conversationActions } from "./conversationActionAvailability";
import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import {
  ConversationActionContext,
  ConversationRequestContext,
} from "./forkChatRequest";

export function UserMessageActions({
  page,
  turn,
  text,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
  text: string;
}) {
  const requestFork = useContext(ConversationRequestContext);
  const context = useContext(ConversationActionContext);
  const actions = conversationActions(context ?? page, turn);
  if (!actions.rewind && !actions.fork) return null;
  return (
    <div
      className="chat-transcript-actions"
      role="group"
      aria-label="Prompt actions"
    >
      {actions.rewind && requestFork ? (
        <button
          type="button"
          className="button--compact"
          onClick={() =>
            requestFork({
              kind: "rewind",
              chatId: page.chatId,
              turnId: turn.id,
              text,
            })
          }
        >
          Rewind here
        </button>
      ) : null}
      {actions.fork && requestFork ? (
        <button
          type="button"
          className="button--compact"
          onClick={() =>
            requestFork({
              chatId: page.chatId,
              turnId: turn.id,
              keyboard: false,
            })
          }
        >
          Fork from here
        </button>
      ) : null}
    </div>
  );
}
