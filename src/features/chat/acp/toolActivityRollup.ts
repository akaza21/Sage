import type { AcpItem, ToolPresentation } from "../../../domain/acp";
import { terminalToolStates } from "./statusSemantics";
import { commandRowText } from "./toolCommandContent";
import {
  collaborationRun,
  subagentToolSummary,
  type CollaborationProjectionItem,
} from "./subagentActivity";

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;
type ToolKind = ToolPresentation["kind"];

const MAX_TOOL_ROLLUP_ITEMS = 64;
const COMPACTED = "Context automatically compacted";
const WEB_SEARCH_INTEGRATION = "Search Service";

export type ToolRollup = {
  kind: ToolKind | "mixed";
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

const fileVerbs: Partial<Record<ToolKind, readonly string[]>> = {
  read: ["Reading", "Read", "read a file", "read files"],
  edit: ["Editing", "Edited", "edited a file", "edited files"],
  delete: ["Deleting", "Deleted", "deleted a file", "deleted files"],
  move: ["Moving", "Moved", "moved a file", "moved files"],
};

const clauses: Partial<Record<ToolKind, readonly string[]>> = {
  execute: ["ran a command", "ran commands"],
  imageView: ["viewed an image", "viewed images"],
  search: ["explored files", "explored files"],
  fetch: ["fetched a page", "fetched pages"],
};

function itemFile(presentation: ToolPresentation): string | null {
  const location =
    "resources" in presentation ? presentation.resources[0] : undefined;
  return location?.kind === "resource"
    ? (location.resource.relativePath.split(/[\\/]/).pop() ?? null)
    : null;
}

/** Latest live call includes waiting/unstarted calls unless the Turn ended (s6/f0526). */
export function liveToolItem(
  items: readonly ToolProjectionItem[],
  turnEnded = false,
): ToolProjectionItem | undefined {
  if (turnEnded) return undefined;
  return [...items]
    .reverse()
    .find((item) => !terminalToolStates.has(item.toolState));
}

export function liveToolProgress(
  item: ToolProjectionItem | undefined,
): string | null {
  const presentation = item?.payload.presentation;
  return presentation?.kind === "mcp" && item?.toolState === "inProgress"
    ? (presentation.progress ?? null)
    : null;
}

export function toolItemLabel(
  item: ToolProjectionItem,
  live = Boolean(liveToolItem([item])),
  tests = commandActivity(item) === "declaredTest",
): string {
  const presentation = item.payload.presentation;
  const verbs = fileVerbs[presentation.kind];
  if (verbs) {
    const file = itemFile(presentation);
    return file ? `${verbs[live ? 0 : 1]} ${file}` : presentation.label;
  }
  switch (presentation.kind) {
    case "execute": {
      const command = toolCommandLine(item);
      const verb = live ? "Running" : "Ran";
      return tests
        ? `${verb} tests${command ? ` · ${command}` : ""}`
        : `${verb} ${command ?? "a command"}`;
    }
    case "imageView":
      return live ? "Viewing an image" : "Viewed an image";
    case "webSearch":
      return "Web run";
    case "mcp":
      return presentation.toolLabel;
    case "think":
      return live ? "Thinking" : "Thought";
    default:
      return presentation.label;
  }
}

function mixedClauses(items: readonly ToolProjectionItem[]): string {
  const counts = new Map<string, number>();
  const integrations = new Set<string>();
  for (const { payload } of items) {
    const presentation = payload.presentation;
    const integration =
      presentation.kind === "mcp"
        ? presentation.serverLabel
        : presentation.kind === "webSearch"
          ? WEB_SEARCH_INTEGRATION
          : null;
    if (integration) integrations.add(integration);
    const key = integration ? "" : presentation.kind;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const names = [...integrations];
  const text = [...counts]
    .map(([kind, count]) => {
      if (!kind)
        return names.length === 1
          ? `used ${names[0]} integration`
          : `used ${names.slice(0, -1).join(", ")} and ${names.at(-1)} integrations`;
      const forms = clauses[kind as ToolKind] ??
        fileVerbs[kind as ToolKind]?.slice(2) ?? ["used a tool", "used tools"];
      return forms[count === 1 ? 0 : 1];
    })
    .join(", ");
  return text[0].toUpperCase() + text.slice(1);
}

export function toolRollupLabel(
  kind: ToolRollup["kind"],
  items: readonly ToolProjectionItem[],
  activity: ToolRollup["commandActivity"] = "ordinary",
  turnEnded = false,
): string {
  if (kind === "subagent")
    return items.every(
      (item) =>
        item.payload.presentation.kind === "subagent" &&
        item.payload.presentation.taskLifecycle === true,
    )
      ? subagentToolSummary(items)
      : items.length === 1
        ? items[0].payload.presentation.label
        : `Ran ${items.length} subagents`;
  const tests = activity === "declaredTest";
  const current = liveToolItem(items, turnEnded);
  if (items.length === 1)
    return toolItemLabel(items[0], current === items[0], tests);
  if (current) return toolItemLabel(current, true, tests);
  return tests ? `Ran tests · ${items.length} commands` : mixedClauses(items);
}

const commandRows = new Map<string, string>();

export function toolCommandLine(item: ToolProjectionItem): string | null {
  const { presentation, inputSummary } = item.payload;
  if (presentation.kind !== "execute") return null;
  const summary = presentation.summary?.trim() || inputSummary?.trim();
  if (!summary) return null;
  const cached = commandRows.get(summary);
  if (cached !== undefined) return cached;
  const candidate = commandRowText(summary);
  const command = candidate
    .slice(0, 512)
    .replace(/[\r\n\t\u2028\u2029]/g, " ")
    .trim();
  const label =
    candidate.length <= 512
      ? command
      : `${command
          .slice(0, 511)
          .replace(/[\uD800-\uDBFF]$/, "")
          .trimEnd()}…`;
  commandRows.set(summary, label);
  if (commandRows.size > 16)
    commandRows.delete(commandRows.keys().next().value!);
  return label;
}

function joinable(item: ToolProjectionItem): boolean {
  const presentation = item.payload.presentation;
  return presentation.kind !== "subagent" && presentation.label !== COMPACTED;
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

    // Until bindings admit callItemId, only projected toolState is
    // authoritative.
    const toolKind = item.payload.presentation.kind;
    const activity = commandActivity(item);
    const tools = [item];
    cursor += 1;
    while (tools.length < MAX_TOOL_ROLLUP_ITEMS) {
      const candidate = items[cursor];
      if (
        !candidate ||
        !isToolItem(candidate) ||
        commandActivity(candidate) !== activity ||
        !(joinable(item) && joinable(candidate)
          ? true
          : toolKind === "subagent" &&
            candidate.payload.presentation.kind === "subagent" &&
            candidate.payload.presentation.taskLifecycle ===
              item.payload.presentation.taskLifecycle)
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
