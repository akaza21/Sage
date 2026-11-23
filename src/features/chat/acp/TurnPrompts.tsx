import { createContext } from "react";
import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import { NoticePart } from "../lazyParts";
import { turnCommand } from "../../../domain/acpCommand";
import { ItemView } from "./TurnActivityItem";

// Only the current chat provides this; inherited prompts have no actions.
// Controls follow page-wide eligibility even when their Turn is memoized.
export const PromptPageContext = createContext<AcpChatPage | null>(null);

export function renderTurnPrompts(
  items: readonly AcpItem[],
  turn: TurnProjection,
  interactive: boolean,
) {
  // Rows stay unclassed: the lazy actions style them once they mount.
  return items.map((item) => (
    <div key={item.id}>
      <ItemView
        item={item}
        interactive={interactive}
        command={turnCommand(turn)}
      />
      {/* Eligibility is decided in the lazy chunk: zero startup bytes. */}
      <NoticePart part="PromptActions" turn={turn} item={item} />
    </div>
  ));
}
