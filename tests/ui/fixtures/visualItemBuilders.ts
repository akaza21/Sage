import type { AcpItem, SafeResourceLocation } from "../../../src/domain/acp";

import { ROOT_ID } from "./visualFixtureIds";
import type { ItemInput } from "./visualFixtureIds";
import { itemIdentity } from "./visualFixtureIds";

export function textItem(
  input: ItemInput,
  role: "user" | "assistant" | "system",
  text: string,
): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "text",
    toolState: null,
    payload: { role, text },
  };
}

export function thoughtItem(input: ItemInput, summary: string): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "thoughtSummary",
    toolState: null,
    payload: { summary },
  };
}

export function planItem(input: ItemInput): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "plan",
    toolState: null,
    payload: {
      entries: [
        { label: "Inspect the current hierarchy", state: "completed" },
        { label: "Tighten the interaction states", state: "completed" },
        { label: "Verify narrow layouts", state: "inProgress" },
      ],
    },
  };
}

type ToolPresentation = Extract<
  AcpItem,
  { kind: "toolCall" }
>["payload"]["presentation"];

const FIXTURE_RESOURCE: SafeResourceLocation = {
  kind: "resource",
  resource: { rootId: ROOT_ID, relativePath: "src/main.ts" },
};

export const TOOL_KIND_PRESENTATIONS = {
  imageView: {
    kind: "imageView",
    label: "View src/diagram.png",
    resources: [
      {
        kind: "resource",
        resource: { rootId: ROOT_ID, relativePath: "src/diagram.png" },
      },
    ],
  },
  webSearch: {
    kind: "webSearch",
    label: "Search web",
  },
  read: {
    kind: "read",
    label: "Read src/main.ts",
    resources: [FIXTURE_RESOURCE],
  },
  edit: {
    kind: "edit",
    label: "Edit [redacted]",
    resources: [FIXTURE_RESOURCE],
  },
  delete: {
    kind: "delete",
    label: "Delete src/stale.ts",
    resources: [FIXTURE_RESOURCE],
  },
  move: {
    kind: "move",
    label: "Move src/old.ts",
    resources: [FIXTURE_RESOURCE],
    originalResources: [FIXTURE_RESOURCE],
  },
  search: {
    kind: "search",
    label: "Search workspace",
    querySummary: "interaction states",
    resources: [],
    matchCount: 14,
    truncated: false,
  },
  execute: {
    kind: "execute",
    label: "Run checks",
    summary: "npm run check",
    cwd: FIXTURE_RESOURCE,
    exitCode: 0,
    outputTail: "12 passed",
    outputTruncated: false,
    resources: [],
    activity: "ordinary",
  },
  fetch: {
    kind: "fetch",
    label: "Fetch changelog",
    url: "https://example.invalid/changelog",
    summary: "200 OK",
  },
  think: {
    kind: "think",
    label: "Consider the failing assertion",
    summary: "Weighing two causes",
  },
  switchMode: {
    kind: "switchMode",
    label: "Entered review mode",
    modeId: null,
    modeLabel: null,
  },
  mcp: {
    kind: "mcp",
    label: "Call linter server",
    serverLabel: "lint-mcp",
    toolLabel: "check",
    argumentsSummary: "{}",
    resultSummary: "clean",
  },
  subagent: {
    kind: "subagent",
    label: "Delegate to reviewer",
    activityId: null,
  },
  other: {
    kind: "other",
    label: "harness.future_thing",
    rawKind: "harness.future_thing",
    summary: null,
  },
} satisfies {
  [Kind in ToolPresentation["kind"]]: ToolPresentation & { kind: Kind };
};

export function toolItem(
  input: ItemInput,
  state: "completed" | "inProgress",
  presentation: ToolPresentation = TOOL_KIND_PRESENTATIONS.search,
): Extract<AcpItem, { kind: "toolCall" }> {
  const identity = itemIdentity(input);
  return {
    ...identity,
    lifecycle: state === "completed" ? "completed" : "streaming",
    terminalAt: state === "completed" ? identity.terminalAt : null,
    kind: "toolCall",
    toolState: state,
    payload: {
      presentation,
      inputSummary: "Scanning relevant UI modules",
    },
  } as Extract<AcpItem, { kind: "toolCall" }>;
}

export function errorItem(input: ItemInput): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "error",
    toolState: null,
    lifecycle: "failed",
    payload: {
      code: "PREVIEW_UNAVAILABLE",
      message:
        "The preview process stopped unexpectedly. The Chat is preserved and can be retried.",
      recoverable: true,
    },
  };
}

export function fileChangeItem(input: ItemInput): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "fileChangeDiff",
    toolState: null,
    payload: {
      changes: [
        {
          resource: {
            kind: "resource",
            resource: {
              rootId: ROOT_ID,
              relativePath: "src/workbench/AgentChatPanes.tsx",
            },
          },
          operation: "updated",
          originalResource: null,
          summary: "Refined the dual-chat layout",
          additions: 28,
          deletions: 11,
        },
        {
          resource: {
            kind: "resource",
            resource: {
              rootId: ROOT_ID,
              relativePath: "src/styles/workbench.css",
            },
          },
          operation: "updated",
          originalResource: null,
          summary: "Aligned spacing and focus states",
          additions: 16,
          deletions: 7,
        },
      ],
      truncated: false,
    },
  };
}
