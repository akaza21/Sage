import { Fragment, useContext, useMemo, type ReactNode } from "react";
import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import { ConversationPart } from "../lazyParts";
import {
  conversationActions,
  conversationState,
  ConversationActionContext,
} from "../forkChatRequest";
import { turnCommand } from "../../../domain/acpCommand";
import { ItemView } from "./TurnActivityItem";

export function TurnPrompts({
  items,
  turn,
  interactive,
  page,
}: {
  items: readonly AcpItem[];
  turn: TurnProjection;
  interactive: boolean;
  page: AcpChatPage;
}) {
  const command = turnCommand(turn);
  const context = useContext(ConversationActionContext);
  const actions = interactive
    ? conversationActions(page, turn, context ?? undefined)
    : { rewind: false, fork: false };
  return items.map((item) => (
    <Fragment key={item.id}>
      <ItemView item={item} interactive={interactive} command={command} />
      {interactive &&
      (actions.rewind || actions.fork) &&
      item.kind === "text" ? (
        <ConversationPart
          part="UserMessageActions"
          page={page}
          turn={turn}
          text={item.payload.text}
        />
      ) : null}
    </Fragment>
  ));
}

export function ConversationActions({
  page,
  children,
}: {
  page: AcpChatPage;
  children: ReactNode;
}) {
  const context = useMemo(() => conversationState(page), [page]);
  return (
    <ConversationActionContext.Provider value={context}>
      {children}
    </ConversationActionContext.Provider>
  );
}
