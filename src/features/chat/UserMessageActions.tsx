import { useContext } from "react";
import { conversationActions } from "./conversationActionAvailability";
import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { ConversationRequestContext } from "./forkChatRequest";

export function UserMessageActions({
  page,
  turn,
  text,
  onDismiss,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
  text: string;
  onDismiss: () => void;
}) {
  const requestFork = useContext(ConversationRequestContext);
  const actions = conversationActions(page, turn);
  if (!actions.rewind && !actions.fork)
    return <p role="status">No actions available.</p>;
  return (
    <>
      {actions.rewind && requestFork ? (
        <button
          type="button"
          role="menuitem"
          className="button--compact answer-more-menu__item"
          onClick={() => {
            onDismiss();
            requestFork({
              kind: "rewind",
              chatId: page.chatId,
              turnId: turn.id,
              text,
            });
          }}
        >
          Rewind here
        </button>
      ) : null}
      {actions.fork && requestFork ? (
        <button
          type="button"
          role="menuitem"
          className="button--compact answer-more-menu__item"
          onClick={() => {
            onDismiss();
            requestFork({
              chatId: page.chatId,
              turnId: turn.id,
              keyboard: false,
            });
          }}
        >
          Fork from here
        </button>
      ) : null}
    </>
  );
}
