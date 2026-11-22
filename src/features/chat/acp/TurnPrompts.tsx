import { Fragment } from "react";
import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import { NoticePart } from "../lazyParts";
import { turnCommand } from "../../../domain/acpCommand";
import { currentSession } from "./acpTurnState";
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
  const eligible = !!currentSession(page)?.capabilities?.rewind;
  return items.map((item) => (
    <Fragment key={item.id}>
      <ItemView item={item} interactive={interactive} command={command} />
      {eligible && item.kind === "text" ? (
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
