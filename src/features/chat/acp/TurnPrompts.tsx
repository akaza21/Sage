import { Fragment } from "react";
import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import { ConversationPart } from "../lazyParts";
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
  return items.map((item) => (
    <Fragment key={item.id}>
      <ItemView item={item} interactive={interactive} command={command} />
      {interactive && turn.terminalAt != null && item.kind === "text" ? (
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
