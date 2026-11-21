import { useContext } from "react";
import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import {
  conversationActions,
  ConversationActionContext,
  ForkChatRequestContext,
  RewindChatRequestContext,
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
  const requestFork = useContext(ForkChatRequestContext);
  const requestRewind = useContext(RewindChatRequestContext);
  const context = useContext(ConversationActionContext);
  const actions = conversationActions(page, turn, context ?? undefined);
  return (
    <div
      className="chat-transcript-actions"
      role="group"
      aria-label="Prompt actions"
    >
      {actions.rewind && requestRewind ? (
        <button
          type="button"
          className="button--compact"
          onClick={() =>
            requestRewind({ chatId: page.chatId, turnId: turn.id, text })
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
