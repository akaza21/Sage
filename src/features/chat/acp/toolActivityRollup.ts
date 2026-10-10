import type { AcpItem, ToolPresentation } from "../../../domain/acp";
import { terminalToolStates, toolStateLabels } from "./statusSemantics";
import {
  collaborationRun,
  subagentToolSummary,
  type CollaborationProjectionItem,
} from "./subagentActivity";

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;

const MAX_TOOL_ROLLUP_ITEMS = 64;

export type ToolRollup = {
  kind: ToolPresentation["kind"] | "mixed";
  /** Commands only; non-command groups use ordinary. */
  commandActivity: "ordinary" | "declaredTest";
  items: readonly ToolProjectionItem[];
};

export type PrimaryActivityItem =
  | { kind: "item"; item: AcpItem }
  | { kind: "toolRollup"; rollup: ToolRollup }
  | {
      kind: "collaborationRollup";
      items: readonly CollaborationProjectionItem[];
    };

function isToolItem(item: AcpItem): item is ToolProjectionItem {
  return item.kind === "toolCall";
}

function commandActivity(
  item: ToolProjectionItem,
): ToolRollup["commandActivity"] {
  const { presentation } = item.payload;
  return presentation.kind === "execute" ? presentation.activity : "ordinary";
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
      (presentation.kind !== "read" &&
        presentation.kind !== "edit" &&
        presentation.kind !== "delete" &&
        presentation.kind !== "move") ||
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
  activity: "Read" | "Edited" | "Deleted" | "Moved",
  items: readonly ToolProjectionItem[],
): string {
  const resourceCount = distinctResourceCount(items);
  return `${activity} ${
    resourceCount === null
      ? countLabel(items.length, "item")
      : countLabel(resourceCount, "file")
  }`;
}

function mixedToolLabel(items: readonly ToolProjectionItem[]): string {
  const kinds = [
    ...new Set(items.map((item) => item.payload.presentation.kind)),
  ];
  return kinds
    .map((kind, index) => {
      const matching = items.filter(
        (item) => item.payload.presentation.kind === kind,
      );
      const file = distinctResourceCount(matching) === 1 ? "a file" : "files";
      const phrase =
        kind === "edit"
          ? `edited ${file}`
          : kind === "read"
            ? `read ${file}`
            : `ran ${countLabel(matching.length, "command")}`;
      return index === 0 ? phrase[0].toUpperCase() + phrase.slice(1) : phrase;
    })
    .join(", ");
}

export function toolRollupLabel(
  kind: ToolRollup["kind"],
  items: readonly ToolProjectionItem[],
  commandActivity: ToolRollup["commandActivity"] = "ordinary",
): string {
  if (kind === "execute") return commandRollupLabel(items, commandActivity);
  const live = items.some((item) => item.toolState === "inProgress");
  if (live && kind === "edit") return "Editing files";
  if (live && kind === "think") return "Thinking";
  switch (kind) {
    case "mixed":
      return mixedToolLabel(items);
    case "read":
      return resourceRollupLabel("Read", items);
    case "edit":
      return resourceRollupLabel("Edited", items);
    case "delete":
      return resourceRollupLabel("Deleted", items);
    case "move":
      return resourceRollupLabel("Moved", items);
    case "search":
      return `Search activity · ${countLabel(items.length, "item")}`;
    case "fetch":
      return `Fetch activity · ${countLabel(items.length, "item")}`;
    case "think":
      return `Thinking · ${countLabel(items.length, "item")}`;
    case "switchMode":
      return `Mode activity · ${countLabel(items.length, "item")}`;
    case "mcp":
      if (items.length === 1) {
        const presentation = items[0]?.payload.presentation;
        if (presentation?.kind === "mcp") {
          return presentation.serverLabel === "MCP"
            ? presentation.toolLabel
            : `${presentation.serverLabel} · ${presentation.toolLabel}`;
        }
      }
      return `MCP activity · ${countLabel(items.length, "tool")}`;
    case "subagent":
      return items.every(
        (item) =>
          item.payload.presentation.kind === "subagent" &&
          item.payload.presentation.taskLifecycle === true,
      )
        ? subagentToolSummary(items)
        : `Subagent activity · ${countLabel(items.length, "item")}`;
    case "other":
      if (
        items.length === 1 &&
        items[0]?.payload.presentation.label ===
          "Context automatically compacted"
      ) {
        return "Context automatically compacted";
      }
      return `Tool activity · ${countLabel(items.length, "item")}`;
  }
}

function commandRollupLabel(
  items: readonly ToolProjectionItem[],
  activity: ToolRollup["commandActivity"],
): string {
  const state = toolRollupState(items);
  const stateCount = items.filter((item) => item.toolState === state).length;
  if (stateCount < items.length) {
    const stateCounts = (
      [
        "inProgress",
        "failed",
        "declined",
        "cancelled",
        "waitingForApproval",
        "pending",
      ] as const
    ).flatMap((toolState) => {
      const count = items.filter((item) => item.toolState === toolState).length;
      if (count === 0) return [];
      const label =
        toolState === "inProgress"
          ? "running"
          : toolState === "waitingForApproval"
            ? "awaiting approval"
            : toolStateLabels[toolState].toLowerCase();
      return [`${count} ${label}`];
    });
    const noun = activity === "declaredTest" ? "test command" : "command";
    return `${countLabel(items.length, noun)} \u00b7 ${stateCounts.join(", ")}`;
  }
  const action =
    state === "completed"
      ? "Ran"
      : state === "inProgress"
        ? "Running"
        : toolStateLabels[state];
  const command = items.length === 1 ? toolCommandLine(items[0]) : null;
  const count = countLabel(items.length, "command");
  if (activity === "declaredTest") {
    const testCommand = items.length === 1 ? "Test command" : "Test commands";
    const label =
      state === "completed" || state === "inProgress"
        ? state === "inProgress" && items.length === 1
          ? "Running test command"
          : `${action} tests`
        : state === "waitingForApproval"
          ? `${testCommand} awaiting approval`
          : `${testCommand} ${action.toLowerCase()}`;
    return `${label} \u00b7 ${command || count}`;
  }
  const label =
    state === "waitingForApproval" ? "Awaiting approval \u00b7" : action;
  return `${label} ${command || (state === "inProgress" && items.length === 1 ? "command" : count)}`;
}

export function toolCommandLine(item: ToolProjectionItem): string | null {
  const { presentation, inputSummary } = item.payload;
  if (presentation.kind !== "execute") return null;
  const candidate = presentation.summary?.trim() || inputSummary?.trim();
  if (!candidate) return null;
  const command = candidate
    .slice(0, 512)
    .replace(/[\r\n\t\u2028\u2029]/g, " ")
    .trim();
  return candidate.length <= 512
    ? command
    : `${command
        .slice(0, 511)
        .replace(/[\uD800-\uDBFF]$/, "")
        .trimEnd()}\u2026`;
}

function canJoinMixed(item: ToolProjectionItem): boolean {
  return (
    item.toolState === "completed" &&
    commandActivity(item) === "ordinary" &&
    (item.payload.presentation.kind === "edit" ||
      item.payload.presentation.kind === "read" ||
      item.payload.presentation.kind === "execute")
  );
}

function presentationDetail(presentation: ToolPresentation): string {
  switch (presentation.kind) {
    case "execute":
    case "other":
    case "fetch":
    case "think":
      return presentation.summary ?? presentation.label;
    case "search":
      return presentation.querySummary || presentation.label;
    case "mcp":
      return presentation.serverLabel === "MCP"
        ? presentation.toolLabel
        : `${presentation.serverLabel} · ${presentation.toolLabel}`;
    default:
      return presentation.label;
  }
}

export function toolRollupLeadDetail(
  rollupLabel: string,
  items: readonly ToolProjectionItem[],
): string | null {
  const first = items[0];
  if (!first) return null;
  const detail = presentationDetail(first.payload.presentation).trim();
  if (detail.length === 0 || detail === rollupLabel.trim()) return null;
  return detail;
}

export function toolRollupState(
  items: readonly ToolProjectionItem[],
): ToolProjectionItem["toolState"] {
  const commands = items.every(
    (item) => item.payload.presentation.kind === "execute",
  );
  const running = items.some((item) => item.toolState === "inProgress");
  if (commands && running) return "inProgress";
  if (items.some((item) => item.toolState === "failed")) return "failed";
  if (items.some((item) => item.toolState === "declined")) return "declined";
  if (items.some((item) => item.toolState === "cancelled")) return "cancelled";
  if (
    running ||
    (!commands && items.some((item) => !terminalToolStates.has(item.toolState)))
  )
    return "inProgress";
  if (items.some((item) => item.toolState === "waitingForApproval"))
    return "waitingForApproval";
  if (items.some((item) => item.toolState === "pending")) return "pending";
  return "completed";
}

export function toolRollupStateLabel(
  state: ToolProjectionItem["toolState"],
): string {
  return toolStateLabels[state];
}

export function toolDurationLabel(
  createdAt: number,
  terminalAt: number | null,
): string | null {
  if (terminalAt === null) return null;
  if (!Number.isFinite(createdAt) || !Number.isFinite(terminalAt)) return null;
  const elapsed = terminalAt - createdAt;
  if (elapsed < 0) return null;
  if (elapsed < 1_000) return "<1s";
  const seconds = Math.round(elapsed / 1_000);
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

export function toolRollupDurationLabel(
  items: readonly ToolProjectionItem[],
): string | null {
  if (items.length === 0) return null;
  let started = Number.POSITIVE_INFINITY;
  let ended = Number.NEGATIVE_INFINITY;
  for (const item of items) {
    if (item.terminalAt === null) return null;
    started = Math.min(started, item.createdAt);
    ended = Math.max(ended, item.terminalAt);
  }
  return toolDurationLabel(started, ended);
}

export function groupPrimaryActivities(
  items: readonly AcpItem[],
): readonly PrimaryActivityItem[] {
  const grouped: PrimaryActivityItem[] = [];
  let cursor = 0;
  while (cursor < items.length) {
    const item = items[cursor];
    const collaborators = collaborationRun(items, cursor);
    if (collaborators) {
      grouped.push({ kind: "collaborationRollup", items: collaborators });
      cursor += collaborators.length;
      continue;
    }
    if (!item || !isToolItem(item)) {
      if (item) grouped.push({ kind: "item", item });
      cursor += 1;
      continue;
    }

    // Result linking can later alter a call's effective state; until bindings
    // admit callItemId, the projection's own toolState is the only authority.
    const toolKind = item.payload.presentation.kind;
    const activity = commandActivity(item);
    const tools = [item];
    cursor += 1;
    while (tools.length < MAX_TOOL_ROLLUP_ITEMS) {
      const candidate = items[cursor];
      if (
        !candidate ||
        !isToolItem(candidate) ||
        !(
          (canJoinMixed(item) && canJoinMixed(candidate)) ||
          (candidate.payload.presentation.kind === toolKind &&
            commandActivity(candidate) === activity &&
            (item.payload.presentation.kind !== "subagent" ||
              (candidate.payload.presentation.kind === "subagent" &&
                candidate.payload.presentation.taskLifecycle ===
                  item.payload.presentation.taskLifecycle)))
        )
      )
        break;
      tools.push(candidate);
      cursor += 1;
    }

    grouped.push({
      kind: "toolRollup",
      rollup: {
        kind: tools.some((tool) => tool.payload.presentation.kind !== toolKind)
          ? "mixed"
          : toolKind,
        commandActivity: activity,
        items: tools,
      },
    });
  }
  return grouped;
}
