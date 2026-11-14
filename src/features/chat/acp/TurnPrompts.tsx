import type { AcpItem, TurnProjection } from "../../../domain/acp";
import { turnCommand } from "../../../domain/acpCommand";
import { ItemView } from "./TurnActivityItem";

export function TurnPrompts({
  items,
  turn,
  interactive,
}: {
  items: readonly AcpItem[];
  turn: Pick<TurnProjection, "command" | "sessionAction">;
  interactive: boolean;
}) {
  const command = turnCommand(turn);
  return items.map((item) => (
    <ItemView
      key={item.id}
      item={item}
      interactive={interactive}
      command={command}
    />
  ));
}
