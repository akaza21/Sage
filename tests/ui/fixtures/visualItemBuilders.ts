import type { AcpItem } from "../../../src/domain/acp";

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

export function toolItem(
  input: ItemInput,
  state: "completed" | "inProgress",
): AcpItem {
  const identity = itemIdentity(input);
  return {
    ...identity,
    lifecycle: state === "completed" ? "completed" : "streaming",
    terminalAt: state === "completed" ? identity.terminalAt : null,
    kind: "toolCall",
    toolState: state,
    payload: {
      presentation: {
        kind: "search",
        label: "Search workspace",
        querySummary: "interaction states",
        resources: [],
      },
      inputSummary: "Scanning relevant UI modules",
    },
  };
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
