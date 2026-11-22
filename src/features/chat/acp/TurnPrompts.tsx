import { Fragment } from "react";
import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import { NoticePart } from "../lazyParts";
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
      {turn.conversationTarget && item.kind === "text" ? (
        <NoticePart
          part="PromptActions"
          page={page}
          turn={turn}
          item={item}
          interactive={interactive}
        />
      ) : null}
    </Fragment>
  ));
}
