import type { AcpItem, ToolPresentation } from "../../../domain/acp";
import { terminalToolStates, toolStateLabels } from "./statusSemantics";

type ToolProjectionItem = Extract<
  AcpItem,
  { kind: "toolCall" | "toolUpdateResult" }
>;

const MAX_TOOL_ROLLUP_ITEMS = 64;

export type ToolRollup = {
  kind: ToolPresentation["kind"];
  items: readonly ToolProjectionItem[];
};

export type PrimaryActivityItem =
  { kind: "item"; item: AcpItem } | { kind: "toolRollup"; rollup: ToolRollup };

function isToolItem(item: AcpItem): item is ToolProjectionItem {
  return item.kind === "toolCall" || item.kind === "toolUpdateResult";
}

function countLabel(count: number, singular: string, plural = `${singular}s`) {
  return `${count} ${count === 1 ? singular : plural}`;
}

function distinctResourceCount(
  items: readonly ToolProjectionItem[],
): number | null {
  const pathsByRoot = new Map<string, Set<string>>();
  for (const item of items) {
    const presentation = item.payload.presentation;
    if (
      (presentation.kind !== "file" && presentation.kind !== "diff") ||
      presentation.resources.length === 0 ||
      presentation.resources.some((location) => location.kind === "unavailable")
    ) {
      return null;
    }
    for (const location of presentation.resources) {
      if (location.kind !== "resource") continue;
      const paths =
        pathsByRoot.get(location.resource.rootId) ?? new Set<string>();
      paths.add(location.resource.relativePath);
      pathsByRoot.set(location.resource.rootId, paths);
    }
  }
  return [...pathsByRoot.values()].reduce(
    (count, paths) => count + paths.size,
    0,
  );
}

function resourceRollupLabel(
  activity: "File" | "Diff",
  items: readonly ToolProjectionItem[],
): string {
  const resourceCount = distinctResourceCount(items);
  return `${activity} activity · ${
    resourceCount === null
      ? countLabel(items.length, "item")
      : countLabel(resourceCount, "file")
  }`;
}

export function toolRollupLabel(
  kind: ToolPresentation["kind"],
  items: readonly ToolProjectionItem[],
): string {
  switch (kind) {
    case "command":
      return `Ran ${countLabel(items.length, "command")}`;
    case "file":
      return resourceRollupLabel("File", items);
    case "diff":
      return resourceRollupLabel("Diff", items);
    case "search":
      return `Search activity · ${countLabel(items.length, "item")}`;
    case "web":
      return `Web activity · ${countLabel(items.length, "item")}`;
    case "mcp":
      return `MCP activity · ${countLabel(items.length, "item")}`;
    case "subagent":
      return `Subagent activity · ${countLabel(items.length, "item")}`;
    case "generic":
      return `Tool activity · ${countLabel(items.length, "item")}`;
  }
}

export function toolRollupState(
  items: readonly ToolProjectionItem[],
): ToolProjectionItem["toolState"] {
  if (items.some((item) => item.toolState === "failed")) return "failed";
  if (items.some((item) => item.toolState === "declined")) return "declined";
  if (items.some((item) => item.toolState === "cancelled")) return "cancelled";
  if (items.some((item) => !terminalToolStates.has(item.toolState)))
    return "inProgress";
  return "completed";
}

export function toolRollupStateLabel(
  state: ToolProjectionItem["toolState"],
): string {
  return toolStateLabels[state];
}

export function groupPrimaryActivities(
  items: readonly AcpItem[],
): readonly PrimaryActivityItem[] {
  const grouped: PrimaryActivityItem[] = [];
  let cursor = 0;
  while (cursor < items.length) {
    const item = items[cursor];
    if (!item || !isToolItem(item)) {
      if (item) grouped.push({ kind: "item", item });
      cursor += 1;
      continue;
    }

    // Result linking can later alter a call's effective state; until bindings
    // admit callItemId, the projection's own toolState is the only authority.
    const toolKind = item.payload.presentation.kind;
    const tools = [item];
    cursor += 1;
    while (tools.length < MAX_TOOL_ROLLUP_ITEMS) {
      const candidate = items[cursor];
      if (
        !candidate ||
        !isToolItem(candidate) ||
        candidate.payload.presentation.kind !== toolKind
      )
        break;
      tools.push(candidate);
      cursor += 1;
    }

    if (tools.length === 1) grouped.push({ kind: "item", item });
    else
      grouped.push({
        kind: "toolRollup",
        rollup: { kind: toolKind, items: tools },
      });
  }
  return grouped;
}
