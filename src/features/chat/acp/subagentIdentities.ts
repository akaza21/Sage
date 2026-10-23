import type { AcpItem } from "../../../domain/acp";
import { collaborationRows } from "./subagentActivity";

function identity(item: AcpItem, key: string) {
  return [
    item.projectId,
    item.chatId,
    item.sessionId,
    item.turnId,
    item.runtimeEpoch,
    key,
  ].join("\0");
}

export function subagentIdentities(items: readonly AcpItem[]) {
  const identities = new Map<string, string>();
  for (const item of items.slice(0, 1_000)) {
    if (item.kind === "collaboration") {
      for (const row of collaborationRows(item)) {
        for (const entry of row.entries) {
          const key = identity(item, entry.key);
          if (identities.size >= 300 && !identities.has(key)) continue;
          identities.set(key, entry.label || identities.get(key) || "");
        }
      }
    } else if (
      item.kind === "toolCall" &&
      item.payload.presentation.kind === "subagent" &&
      identities.size < 300
    ) {
      identities.set(
        identity(item, item.payload.presentation.activityId ?? item.id),
        item.payload.presentation.label,
      );
    }
  }
  return identities;
}

export function withSubagentNames(
  items: readonly AcpItem[],
): readonly AcpItem[] {
  const identities = subagentIdentities(items);
  return items.map((item) => {
    if (item.kind !== "collaboration" || !item.payload.agentKeys?.length)
      return item;
    const receiverLabels = item.payload.agentKeys.map(
      (key, index) =>
        (item.payload.receiverLabels.length === item.payload.agentKeys?.length
          ? item.payload.receiverLabels[index]?.trim()
          : "") ||
        identities.get(identity(item, key)) ||
        "",
    );
    return receiverLabels.some(
      (label, index) => label !== item.payload.receiverLabels[index],
    )
      ? { ...item, payload: { ...item.payload, receiverLabels } }
      : item;
  });
}
