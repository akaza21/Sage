import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../../../src/app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../../../src/app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../../src/app/agentArtifactFollowStore";
import { useWorkbenchStore } from "../../../src/app/store";
import { diffCacheKey } from "../../../src/domain/diffIdentity";
import {
  childResource,
  resourceKey,
} from "../../../src/domain/resourceIdentity";
import type {
  AcpCapabilitySet,
  AcpChatPage,
  AcpItem,
  AgentSessionProjection,
  EntityRevision,
  HarnessActivityProjection,
  ProjectGeneration,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageItemId,
  SageProjectId,
  SageTurnId,
  TurnProjection,
  TurnState,
} from "../../../src/domain/acp";
import type { AgentArtifactOwner } from "../../../src/domain/agentArtifact";
import type {
  EditorDocument,
  FileDiff,
  ResourceRef,
  WorkbenchTab,
} from "../../../src/domain/workbench";
import { workspaceSearchDesktop } from "../../../src/features/search/workspaceSearchDesktop";
import {
  applyBrowseIntent,
  clearBrowseSessions,
  setHarnessBrowseOwner,
} from "../../../src/features/preview/harnessBrowse";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { languageDesktop } from "../../../src/services/languageDesktop";

export type VisualScenarioName =
  | "agent-single"
  | "agent-dual"
  | "agent-long"
  | "agent-activity-family"
  | "agent-image"
  | "agent-child-activity"
  | "editor"
  | "file"
  | "diff"
  | "preview"
  | "error"
  | "waiting";

export type VisualScenarioMode = "agent" | "editor";
export type VisualScenarioResource = "empty" | "file" | "diff" | "preview";

export interface VisualScenarioDefinition {
  name: VisualScenarioName;
  mode: VisualScenarioMode;
  resource: VisualScenarioResource;
}

export const VISUAL_SCENARIOS: readonly VisualScenarioDefinition[] = [
  { name: "agent-single", mode: "agent", resource: "empty" },
  { name: "agent-dual", mode: "agent", resource: "empty" },
  { name: "agent-long", mode: "agent", resource: "empty" },
  { name: "agent-activity-family", mode: "agent", resource: "empty" },
  { name: "agent-image", mode: "agent", resource: "empty" },
  { name: "agent-child-activity", mode: "agent", resource: "empty" },
  { name: "editor", mode: "editor", resource: "file" },
  { name: "file", mode: "agent", resource: "file" },
  { name: "diff", mode: "agent", resource: "diff" },
  { name: "preview", mode: "agent", resource: "preview" },
  { name: "error", mode: "agent", resource: "empty" },
  { name: "waiting", mode: "agent", resource: "empty" },
];

const FIXED_NOW = Date.UTC(2026, 7, 31, 9, 30, 0);
const PROJECT_ID = "visual-project" as SageProjectId;
const ROOT_ID = "visual-root";
const GENERATION = "0" as ProjectGeneration;
const RUNTIME_EPOCH = "7" as RuntimeEpoch;
const SESSION_ID = "visual-session" as SageAgentSessionId;
const BINDING_ID = "visual-binding";
const MODEL_ID = "visual-model";
const VISUAL_AGENT_IMAGE_PNG = [
  "iVBORw0KGgoAAAANSUhEUgAABLAAAAKjCAMAAAA6bQbyAAAAG1BMVEUOERYUGCAqMUAaHyhMjf+0vcmLlaVo2YPvbWdPEC1lAAAJwElEQVR42u3bOQ6AMAxFwbBz/xNDjZQG4UDMzA1ifb0upQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAoAEgiGABggUgWIBgCRYgWACCBQiWYAGCBSBYgGAJFiBYAIIF",
  "CJZgAYIFIFiAYAkWIFgAggUIlmABggUgWIBgCRYgWACCBQiWYAGCBSBYgGA5KSBYgGAJFiBYAIIFCJZgAYIFIFiAYAkWIFgAggUIlmABggUgWIBgCRYgWACCBQiWYAGCBSBYgGAJFiBYAIIFCJZgAYIFIFiAYAkWIFgAggUIFoBgAYIlWIBgAQgWIFiCBQgWgGABgiVYgGABCBYgWIIFCBaAYAGCJVhAxmABAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
  "AAAAAAAA/Rp5XO3WE/yMYAkWCBaCBYIlWIKFYAmWYIFgIVggWIJlvgiWYAkWCBaCBYIlWCBYgiVYIFgIFgiWYIFgCZZggWAhWCBYggWCJViCBYKFYIFgCRYIlmAJFggWggWCJVggWIIlWCBYCBYIlmCBYAmWYIFgIVggWIIFgiUvggWCJViChWAJlmCBYCFYIFiCJVgIlmAJFggW7YM104SACBaCJVgIlmAhWIIlWIIlWAgWgiVYCJZgIViCJViC",
  "JVgIlmAJlmAhWIKFYAkWgiVYCJZgCZZgCZZgCRaCJVgIlmAhWIIlWIIlWIIlWAiWYCFYgoVgCZZgCZZgCZZgIViChWAJFoIlWIIlWIIlWIKFYAkWgiVYCJZgCZZgCZZgCRaCJVhEBAsES7AECwQLwQLBEiwQLMESLBAsBAsES7BAsARLsECwECwQLMECwRIswQLBQrBAsAQLBEuwBAsES7AECwRLsECwECwQLMESLARLsAQLBAvBAsESLPNFsARL",
  "sECwECwQLMECwRIswQLBQrBAsAQLBEuwBAsEC8ECwRIsECzB6iVYC7xIsARLsBAswRIsECwEC8ESLMESLARLsAQLBAvBQrAES7AEC8ESLMECwUKwECzBEizBQrAES7BAsIgPFvj8LFiCBYKFYIFgCRYIlmAJFggWggWCJVggWIIlWCBYCBYIlmCBYAmWYIFgIVggWIIFgiVYggWCJViCBYIlWCBYCBYIlmAJFoIlWIIFgoVggWAJlvkiWIIlWCBY",
  "tAvWCifBEizBQrAEC8FCsARLsBAswRIswUKwBAvBQrAES7AQLMESLMFCsAQLwUKwBEuwECzBEizBQrAES7AEC8ESrLzBAp+fBUuwQLAQLBAswQLBEizBAsFCsECwBAsES7AECwQLwQLBEiwQLMESLBAsBAsES7BAsARLsECwBEuwQLAECwQLwQLBEizBQrAES7BAsBAsECzBMl8ES7AECwQLwQLBEiwQLMESLBAsBAsES7BAsARLsECwECwQLMEC",
  "wRIswQLBQrBAsAQLBEuwBAsEC8ECwRIsECzBEiwQLAQLBEuwQLAES7BAsBAsECzBAsGSF8ECwRIswUKwBEuwQLAQLBAswRIsBEuwBAsEC8ECwRIsECzBEiwQLAQLBEuwrrbE6q/eP05uBEuwBEuwBEuwBEuwBEuwECzBQrAES7AES7AES7AES7AEC8ESLARLsARLsARLsARLsARLsBAswUKwBEuwBEuwBEuwBEuwBAvBEiwES7AES7AES7AES7AE",
  "S7AQLMFCsARLsARLsARLsARLsASLu8ECwRIswQLBQrBAsAQLBEuwBAsEC8ECwRIsECzBEiwQLAQLBEuwQLAES7BAsBAsECzBAsESLMECwRIswQLBEiwQLAQLBEuwBAvBEizBAsFCsECwBMt8ESzBEiwQLAQLBEuwQLAES7BAsBAsECzBAsESLMECwUKwQLAECwRLsAQLBAvBAsESLBAswRIsECwECwRLsECwBEuwQLAQLBAswQLBEizBAsFCsECw",
  "BAsES14ECwRLsAQLwRIswQLBQrBAsARLsBAswRIsECwECwRLsECwBEuwQLAQLBAswQLBEizBAsFCsECwBAsES7AECwQLwQLBEiwQLMESLBAsBAsES7BAsARLsECwECwQLMECwRIswQLBQrBAsAQLBEuwBAsEC8ECwRIsECwECwRLsAQLwRIswQLBQrBAsARLsBAswRIsECwECwRLsECwBEuwQLAQLBAswQLBEizBAsFCsECwBAsES7AECwQLwQLB",
  "EiwQLMESLBAsBAsES7BAsARLsECwECwQLMECwRIswQLBQrBAsAQLBEuwBAsEC8ECwRIsECwECwRLsAQLwRIswQLBQrBAsARLsBAswRIsECwECwRLsECwBEuwQLAQLBAswQLBEizBAsFCsECwBAsES7AECwQLwQLBEiwQLMESLBAsBAsES7BAsARLsECwECwQLMECwRIswQLBQrBAsAQLBEuwBAsES7AECwRLsECwECwQLMESLARLsAQLBAvBAsES",
  "LMFCsARLsECwECwQLMECwRIswQLBQrBAsAQLBEuwBAsEC8ECwRIsECzBEiwQLAQLBEuwQLAES7BAsBAsECzBAsESLMECwUKwQLAECwRLsAQLBAvBAsESLBAswRIsECzBEiwQLMECwUKwQLAES7AQLMESLBAsBAsES7AEC8ESLMECwUKwQLAECwRLsAQLBAvBAsESLBAswRIsECwECwRLsECwBEuwQLAQLBAswQLBEizBAsFCsECwBAsES7AECwQL",
  "wQLBEiwQLMESLBAsBAsES7BAsARLsECwBEuwQLAECwQLwQLBEizBQrAES7BAsBAsECzBEiwES7AECwQLwQLBEiwQLMESLBAsBAsES7BAsARLsECwECwQLMECwRIswQLBQrBAsAQLBEuwBAsEC8ECwRIsECzBEiwQLAQLBEuwQLAES7BAsBAsECzBAsESLMECwRIswQLBEiwQLAQLBEuwBAvBEizBAsFCsECwBEuwECzBEiwQLAQLBEuwQLAES7BA",
  "sBAsECzBAsESLMECwUKwQLAECwRLsAQLBAvBAsESLBAswRIsECwECwRLsECwBEuwQLAQLBAswQLBEizBAsFCsECwBAsES7AECwRLsAQLBEuwQLAQLBAswRIsBEuwBAsEC8ECwRIswUKwBEuwQLAQLBAswQLBEizBAsFCsECwBAsES7AECwQLwQLBEiwQLMESLBAsBAsES7BAsARLsECwECwQLMECwRIswQLBQrBAsAQLBEuwBAsEC8ECwRIsECwA",
  "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAHI4AExmt0aynMpyAAAAAElFTkSuQmCC",
].join("");

const capabilities: AcpCapabilitySet = {
  schemaVersion: 1,
  protocolVersion: "1.0",
  newSession: true,
  resumeSession: true,
  loadSession: true,
  closeSession: true,
  prompt: true,
  cancel: true,
  nativeSessionFork: false,
  activeTurnSteer: false,
  childConversationProjection: true,
  childTranscriptRead: true,
  collaborationEventProjection: true,
  multiRootExtensionSchemaId: null,
  modelCount: 1,
  configCount: 1,
};

function session(state: AgentSessionProjection["state"] = "ready") {
  return {
    id: SESSION_ID,
    ordinal: 1,
    bindingId: BINDING_ID,
    modelId: MODEL_ID,
    state,
    residency: "hot",
    runtimeEpoch: RUNTIME_EPOCH,
    nextRuntimeEpoch: null,
    capabilities,
    revision: "1",
    projectionRevision: "1",
    legacy: false,
  } as AgentSessionProjection;
}

interface ItemInput {
  chatId: SageChatId;
  turnId: SageTurnId;
  ordinal: number;
  createdAt: number;
}

function itemIdentity(input: ItemInput) {
  const id = `${input.turnId}-item-${input.ordinal}` as SageItemId;
  return {
    id,
    projectId: PROJECT_ID,
    chatId: input.chatId,
    sessionId: SESSION_ID,
    turnId: input.turnId,
    runtimeEpoch: RUNTIME_EPOCH,
    ordinal: input.ordinal,
    origin: "primary" as const,
    activityId: null,
    lifecycle: "completed" as const,
    revision: "1" as EntityRevision,
    createdAt: input.createdAt,
    updatedAt: input.createdAt + 1_000,
    terminalAt: input.createdAt + 1_000,
  };
}

function textItem(
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

function thoughtItem(input: ItemInput, summary: string): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "thoughtSummary",
    toolState: null,
    payload: { summary },
  };
}

function planItem(input: ItemInput): AcpItem {
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

function toolItem(
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

type ToolCallItem = Extract<AcpItem, { kind: "toolCall" }>;

function activityToolItem(
  input: ItemInput,
  presentation: ToolCallItem["payload"]["presentation"],
  state: ToolCallItem["toolState"] = "completed",
): AcpItem {
  const identity = itemIdentity(input);
  const terminal = state !== "pending" && state !== "inProgress";
  return {
    ...identity,
    lifecycle: terminal ? "completed" : "streaming",
    terminalAt: terminal ? identity.terminalAt : null,
    kind: "toolCall",
    toolState: state,
    payload: {
      presentation,
      inputSummary: null,
    },
  } as ToolCallItem;
}

function usageItem(input: ItemInput): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "usage",
    toolState: null,
    payload: {
      inputTokens: 1240,
      outputTokens: 386,
      cachedInputTokens: 240,
      totalTokens: 1626,
    },
  } as AcpItem;
}

function imageItem(input: ItemInput): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "image",
    toolState: null,
    payload: {
      mediaType: "image/png",
      dataUrl: `data:image/png;base64,${VISUAL_AGENT_IMAGE_PNG}`,
    },
  };
}

function collaborationItem(
  input: ItemInput,
  activityId: HarnessActivityProjection["id"],
  label: string,
  lifecycle: "streaming" | "completed",
): AcpItem {
  return {
    ...itemIdentity(input),
    origin: "harnessActivity",
    activityId,
    lifecycle,
    kind: "collaboration",
    toolState: null,
    payload: {
      activityId,
      label,
      summary:
        lifecycle === "streaming"
          ? "Reviewing the bounded result in the originating Turn."
          : "The bounded read-only result is ready for review.",
      receiverLabels: lifecycle === "streaming" ? ["Reviewer"] : [],
    },
  };
}

function unsupportedItem(input: ItemInput): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "unsupported",
    toolState: null,
    payload: {
      category: "extension.future",
      summary: "A bounded harness activity is available as read-only details.",
      truncated: true,
    },
  } as AcpItem;
}

function errorItem(input: ItemInput): AcpItem {
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

function fileChangeItem(input: ItemInput): AcpItem {
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

function turn(
  chatId: SageChatId,
  ordinal: number,
  state: TurnState,
  items: AcpItem[],
): TurnProjection {
  const startedAt = FIXED_NOW - (20 - ordinal) * 60_000;
  const terminal = [
    "completed",
    "interrupted",
    "cancelled",
    "failed",
    "deliveryUnknown",
  ].includes(state);
  return {
    id: `visual-turn-${chatId}-${ordinal}` as SageTurnId,
    sessionId: SESSION_ID,
    runtimeEpoch: RUNTIME_EPOCH,
    ordinal,
    state,
    revision: "1" as EntityRevision,
    createdAt: startedAt,
    updatedAt: startedAt + 20_000,
    terminalAt: terminal ? startedAt + 20_000 : null,
    items,
    activities: [],
  };
}

function turnItems(
  chatId: SageChatId,
  ordinal: number,
  options: { rich?: boolean; working?: boolean; failed?: boolean } = {},
) {
  const turnId = `visual-turn-${chatId}-${ordinal}` as SageTurnId;
  const createdAt = FIXED_NOW - (20 - ordinal) * 60_000;
  const input = (itemOrdinal: number): ItemInput => ({
    chatId,
    turnId,
    ordinal: itemOrdinal,
    createdAt: createdAt + itemOrdinal * 1_000,
  });
  const items: AcpItem[] = [
    textItem(
      input(1),
      "user",
      ordinal === 1
        ? "Review the Agent and Editor surfaces and make the experience feel coherent."
        : `Check refinement pass ${ordinal} and keep the existing mode architecture intact.`,
    ),
  ];
  if (options.rich || options.working) {
    items.push(
      thoughtItem(
        input(2),
        "I’m comparing the hierarchy, density, and state language before changing the presentation.",
      ),
    );
  }
  if (options.rich) {
    items.push(planItem(input(3)), toolItem(input(4), "completed"));
  }
  if (options.working) {
    items.push(toolItem(input(3), "inProgress"));
  } else if (options.failed) {
    items.push(errorItem(input(3)));
  } else {
    items.push(
      textItem(
        input(options.rich ? 5 : 2),
        "assistant",
        options.rich
          ? "The core hierarchy is now consistent.\n\n```ts\nconst panes = Math.min(openChats.length, 2);\n```\n\nThe next pass verifies focus, narrow width, and reduced motion."
          : "This pass is stable. Spacing, actions, and state labels remain readable without moving the underlying product seams.",
      ),
    );
  }
  if (options.rich) items.push(fileChangeItem(input(6)));
  return items;
}

function page(
  chatIdValue: string,
  title: string,
  turns: TurnProjection[],
  sessionState: AgentSessionProjection["state"] = "ready",
): AcpChatPage {
  const chatId = chatIdValue as SageChatId;
  return {
    projectId: PROJECT_ID,
    chatId,
    title,
    projectionRevision: "42" as ProjectionRevision,
    currentSessionId: SESSION_ID,
    sessions: [session(sessionState)],
    turns,
    nextCursor: null,
  };
}

function standardPage(
  chatIdValue: string,
  title: string,
  variant: "rich" | "simple" | "error" | "waiting",
) {
  const chatId = chatIdValue as SageChatId;
  if (variant === "error") {
    return page(chatId, title, [
      turn(chatId, 1, "failed", turnItems(chatId, 1, { failed: true })),
    ]);
  }
  if (variant === "waiting") {
    return page(
      chatId,
      title,
      [turn(chatId, 1, "waitingTool", turnItems(chatId, 1, { working: true }))],
      "active",
    );
  }
  return page(chatId, title, [
    turn(
      chatId,
      1,
      "completed",
      turnItems(chatId, 1, { rich: variant === "rich" }),
    ),
    turn(chatId, 2, "completed", turnItems(chatId, 2)),
  ]);
}

function longPage() {
  const chatId = "visual-chat-long" as SageChatId;
  const turns = Array.from({ length: 14 }, (_, index) => {
    const ordinal = index + 1;
    const working = ordinal === 14;
    return turn(
      chatId,
      ordinal,
      working ? "running" : "completed",
      turnItems(chatId, ordinal, { rich: ordinal === 1, working }),
    );
  });
  return page(chatId, "Long transcript review", turns, "active");
}

function activityFamilyPage() {
  const chatId = "visual-chat-activity-family" as SageChatId;
  const turnId = `visual-turn-${chatId}-1` as SageTurnId;
  const createdAt = FIXED_NOW - 60_000;
  const input = (ordinal: number): ItemInput => ({
    chatId,
    turnId,
    ordinal,
    createdAt: createdAt + ordinal * 1_000,
  });
  const items: AcpItem[] = [
    thoughtItem(
      input(1),
      "Comparing the activity family before the final pass.",
    ),
    planItem(input(2)),
    activityToolItem(input(3), {
      kind: "command",
      label: "Run focused checks",
      summary:
        "npm test -- src/features/chat/acp/ToolItem.test.tsx src/features/chat/acp/TurnActivityItem.test.tsx src/features/chat/acp/AcpChatProjection.test.tsx",
    }),
    activityToolItem(
      input(4),
      {
        kind: "search",
        label: "Search workspace",
        querySummary: "activity card ownership",
        resources: [
          {
            rootId: ROOT_ID,
            relativePath: "src/features/chat/acp/ToolItem.tsx",
          },
        ],
      },
      "inProgress",
    ),
    activityToolItem(input(5), {
      kind: "file",
      label: "Read file",
      resources: [
        {
          kind: "resource",
          resource: {
            rootId: ROOT_ID,
            relativePath: "src/features/chat/acp/AcpChatProjection.tsx",
          },
        },
      ],
    }),
    activityToolItem(input(6), {
      kind: "web",
      label: "Review preview",
      summary: "Opened http://127.0.0.1:1431/",
    }),
    activityToolItem(input(7), {
      kind: "generic",
      label: "Inspect extension state",
      summary: "Normalized bounded activity",
    }),
    usageItem(input(8)),
    errorItem(input(9)),
    unsupportedItem(input(10)),
  ];
  return page(
    chatId,
    "Activity card polish",
    [turn(chatId, 1, "running", items)],
    "active",
  );
}

function imagePage() {
  const chatId = "visual-chat-image" as SageChatId;
  const turnId = `visual-turn-${chatId}-1` as SageTurnId;
  const createdAt = FIXED_NOW - 60_000;
  const input = (ordinal: number): ItemInput => ({
    chatId,
    turnId,
    ordinal,
    createdAt: createdAt + ordinal * 1_000,
  });
  return page(chatId, "Review agent image", [
    turn(chatId, 1, "completed", [
      textItem(
        input(1),
        "assistant",
        "Here is the visual result from the completed pass.",
      ),
      imageItem(input(2)),
    ]),
  ]);
}

function childActivityPage() {
  const chatId = "visual-chat-child-activity" as SageChatId;
  const turnId = `visual-turn-${chatId}-1` as SageTurnId;
  const createdAt = FIXED_NOW - 60_000;
  const input = (ordinal: number): ItemInput => ({
    chatId,
    turnId,
    ordinal,
    createdAt: createdAt + ordinal * 1_000,
  });
  const activityIds = [1, 2, 3, 4].map(
    (level) =>
      `visual-child-activity-${level}` as HarnessActivityProjection["id"],
  );
  const labels = [
    "Coordinate the independent accessibility review across every visible workbench surface",
    "Inspect the responsive hierarchy and preserve every admitted child activity relationship",
    "Verify keyboard disclosure and focus restoration without scheduling any new destination",
    "Summarize the final read-only collaboration result for the originating conversation",
  ] as const;
  const collaborationItems = activityIds.map((activityId, index) =>
    collaborationItem(
      input(index + 2),
      activityId,
      labels[index],
      index === 2 ? "streaming" : "completed",
    ),
  );
  const activities = activityIds.map(
    (activityId, index): HarnessActivityProjection => ({
      id: activityId,
      turnId,
      parentActivityId: index === 0 ? null : activityIds[index - 1],
      parentItemId:
        index === 0
          ? itemIdentity(input(1)).id
          : collaborationItems[index - 1].id,
      role: index === 0 ? "root" : "child",
      lifecycle: index === 2 ? "active" : "completed",
      depth: index + 1,
      provenance: {
        schemaVersion: 1,
        sourceKind: "collaboration",
        externalKind: "child",
        extensionSchemaId: null,
        safeLabel: "Accessibility review activity",
      },
      revision: "1" as EntityRevision,
    }),
  );
  const activityTurn = turn(chatId, 1, "running", [
    textItem(
      input(1),
      "assistant",
      "The harness reported this bounded child-activity hierarchy.",
    ),
    ...collaborationItems,
  ]);
  return page(
    chatId,
    "Child activity review",
    [{ ...activityTurn, activities }],
    "active",
  );
}

export interface BuiltVisualScenario extends VisualScenarioDefinition {
  pages: AcpChatPage[];
}

export function buildVisualScenario(
  name: VisualScenarioName,
): BuiltVisualScenario {
  const definition = VISUAL_SCENARIOS.find((entry) => entry.name === name);
  if (!definition) throw new Error(`Unknown visual scenario: ${name}`);
  const pages =
    name === "agent-dual"
      ? [
          standardPage(
            "visual-chat-primary",
            "Agent navigation polish",
            "rich",
          ),
          standardPage(
            "visual-chat-secondary",
            "Editor consistency pass",
            "waiting",
          ),
        ]
      : name === "agent-long"
        ? [longPage()]
        : name === "agent-activity-family"
          ? [activityFamilyPage()]
          : name === "agent-image"
            ? [
                imagePage(),
                standardPage(
                  "visual-chat-image-review",
                  "Check image result",
                  "waiting",
                ),
              ]
            : name === "agent-child-activity"
              ? [childActivityPage()]
              : name === "error"
                ? [
                    standardPage(
                      "visual-chat-error",
                      "Recover a failed preview",
                      "error",
                    ),
                  ]
                : name === "waiting"
                  ? [
                      standardPage(
                        "visual-chat-waiting",
                        "Inspecting workspace",
                        "waiting",
                      ),
                    ]
                  : [
                      standardPage(
                        `visual-chat-${name}`,
                        name === "preview"
                          ? "Preview interaction"
                          : "Agent navigation polish",
                        "rich",
                      ),
                    ];
  return { ...definition, pages };
}

const fileResource: ResourceRef = {
  rootId: ROOT_ID,
  relativePath: "src/workbench/AgentChatPanes.tsx",
};

function installEditorToolFixtures() {
  languageDesktop.startLanguageSession = () => new Promise<number>(() => {});
  languageDesktop.cancelLanguageSession = async () => undefined;
  languageDesktop.subscribeDiagnostics = async (onReport) => {
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        onReport({
          resource: fileResource,
          generation: 1,
          diagnostics: [
            {
              resource: fileResource,
              message: "ChatSurface must remain scoped to the active Chat",
              severity: "error",
              startLine: 7,
              startColumn: 6,
              endLine: 7,
              endColumn: 17,
            },
            {
              resource: fileResource,
              message: "maximum should be named maxVisibleChats",
              severity: "warning",
              startLine: 4,
              startColumn: 43,
              endLine: 4,
              endColumn: 50,
            },
            {
              resource: fileResource,
              message: "Preview stays bounded to the selected Project",
              severity: "info",
              startLine: 3,
              startColumn: 17,
              endLine: 3,
              endColumn: 31,
            },
          ],
        }),
      ),
    );
    return () => undefined;
  };
  languageDesktop.subscribeOutline = async (onReport) => {
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        onReport({
          resource: fileResource,
          generation: 1,
          symbols: [
            {
              resource: fileResource,
              name: "AgentChatPanes",
              kind: "function",
              startLine: 3,
            },
            {
              resource: fileResource,
              name: "AgentChatPanesWithProjectScopedResourceFollowing",
              kind: "type",
              startLine: 3,
            },
            {
              resource: fileResource,
              name: "visibleChats",
              kind: "unknown",
              startLine: 4,
            },
            {
              resource: fileResource,
              name: "AgentChatPanes.tsx",
              kind: "file",
              startLine: 1,
            },
          ],
        }),
      ),
    );
    return () => undefined;
  };
  workspaceSearchDesktop.search = async (request) => ({
    requestId: "visual-search",
    projectId: request.projectId,
    transitionEpoch: request.expectedTransitionEpoch,
    indexGeneration: request.indexGeneration,
    query: request.query,
    matches: [
      {
        resource: fileResource,
        lineNumber: 3,
        columnNumber: 17,
        preview: "export function AgentChatPanes() {",
      },
    ],
    roots: [
      {
        rootId: ROOT_ID,
        rootName: "Sage",
        state: "complete",
        filesScanned: 124,
        bytesScanned: 84_200,
        entriesVisited: 139,
        skippedFiles: 0,
        errorCount: 0,
        message: null,
      },
    ],
    truncated: false,
    continuationToken: null,
    limitReasons: [],
    elapsedMs: 8,
  });
}

const editorContent = [
  'import { ChatSurface } from "../features/chat/ChatSurface";',
  "",
  "export function AgentChatPanes() {",
  "  const visibleChats = useVisibleChats({ maximum: 2 });",
  "",
  "  return visibleChats.map((chat) => (",
  "    <ChatSurface key={chat.id} chatId={chat.id} />",
  "  ));",
  "}",
].join("\n");

function editorDocument(): EditorDocument {
  const version = {
    fingerprint: "visual-editor-v1",
    byteLen: editorContent.length,
    modifiedAt: FIXED_NOW - 90_000,
  };
  return {
    resource: fileResource,
    disk: version,
    base: version,
    buffer: {
      ...version,
      revision: 0,
      content: editorContent,
    },
    lineEnding: "lf",
    dirty: false,
    conflict: null,
  };
}

const visualDiff: FileDiff = {
  resource: fileResource,
  binary: false,
  truncated: false,
  hunks: [
    {
      header: "@@ -18,7 +18,10 @@ export function AgentChatPanes()",
      lines: [
        {
          kind: "context",
          oldLine: 18,
          newLine: 18,
          text: "const chats = visibleChats();",
        },
        {
          kind: "removed",
          oldLine: 19,
          newLine: null,
          text: "return <ChatSurface chatId={chats[0].id} />;",
        },
        {
          kind: "added",
          oldLine: null,
          newLine: 19,
          text: "return chats.slice(0, 2).map((chat) => (",
        },
        {
          kind: "added",
          oldLine: null,
          newLine: 20,
          text: "  <ChatSurface key={chat.id} chatId={chat.id} />",
        },
        { kind: "added", oldLine: null, newLine: 21, text: "));" },
      ],
    },
  ],
};

function chatTab(pageValue: AcpChatPage, position: number): WorkbenchTab {
  const nonterminal = pageValue.turns.some((candidate) =>
    ["running", "waitingTool", "waitingApproval", "waitingModel"].includes(
      candidate.state,
    ),
  );
  return {
    id: pageValue.chatId,
    kind: "chat",
    title: pageValue.title,
    position,
    pinned: position === 0,
    dirty: false,
    status: nonterminal ? "working" : "complete",
    updatedAt: FIXED_NOW - position * 60_000,
  };
}

function fixtureTabs(scenario: BuiltVisualScenario): WorkbenchTab[] {
  const chats = scenario.pages.map(chatTab);
  if (scenario.resource === "file") {
    return [
      ...chats,
      {
        id: "visual-file-tab",
        kind: "file",
        title: "AgentChatPanes.tsx",
        position: chats.length,
        pinned: false,
        dirty: false,
        resource: fileResource,
      },
    ];
  }
  if (scenario.resource === "diff") {
    return [
      ...chats,
      {
        id: "visual-diff-tab",
        kind: "diff",
        title: "AgentChatPanes.tsx",
        position: chats.length,
        pinned: false,
        dirty: false,
        resource: fileResource,
      },
    ];
  }
  return chats;
}

export async function installVisualScenario(name: VisualScenarioName) {
  const scenario = buildVisualScenario(name);
  const tabs = fixtureTabs(scenario);
  const chatIds = scenario.pages.map((pageValue) => pageValue.chatId);
  const activeTabId =
    scenario.resource === "file"
      ? "visual-file-tab"
      : scenario.resource === "diff"
        ? "visual-diff-tab"
        : (chatIds[0] ?? null);

  clearAcpProjectionRegistry();
  clearBrowseSessions();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  pinAcpProjectionIds(chatIds);
  installEditorToolFixtures();

  const pagesByChat = new Map(
    scenario.pages.map((pageValue) => [pageValue.chatId, pageValue]),
  );
  acpDesktop.getChatPage = async (request) => {
    const pageValue = pagesByChat.get(request.chatId);
    if (!pageValue) throw new Error(`No visual page for ${request.chatId}`);
    return structuredClone(pageValue);
  };

  for (const pageValue of scenario.pages) {
    const scope = {
      projectId: PROJECT_ID,
      projectGeneration: GENERATION,
      chatId: pageValue.chatId,
    };
    const store = acpProjectionStoreFor(pageValue.chatId);
    store.getState().activateScope(scope);
    store.getState().applyAuthoritativePage(scope, structuredClone(pageValue));
  }

  const editorDoc = editorDocument();
  const emptyOwnership = {
    composerText: "",
    sending: false,
    error: null,
    requestGeneration: 0,
  };
  const chatUiById = Object.fromEntries(
    chatIds.map((chatId) => [chatId, { ...emptyOwnership }]),
  );
  const rootListing = {
    resource: { rootId: ROOT_ID, relativePath: "" },
    entries: [
      {
        name: "src",
        resource: { rootId: ROOT_ID, relativePath: "src" },
        kind: "directory" as const,
        byteLen: null,
      },
      {
        name: "README.md",
        resource: { rootId: ROOT_ID, relativePath: "README.md" },
        kind: "file" as const,
        byteLen: 2_048,
      },
    ],
    truncated: false,
  };
  const srcListing = {
    resource: { rootId: ROOT_ID, relativePath: "src" },
    entries: [
      {
        name: "workbench",
        resource: { rootId: ROOT_ID, relativePath: "src/workbench" },
        kind: "directory" as const,
        byteLen: null,
      },
    ],
    truncated: false,
  };
  const workbenchListing = {
    resource: { rootId: ROOT_ID, relativePath: "src/workbench" },
    entries: [
      {
        name: "AgentChatPanes.tsx",
        resource: fileResource,
        kind: "file" as const,
        byteLen: editorContent.length,
      },
    ],
    truncated: false,
  };

  useWorkbenchStore.setState({
    phase: "ready",
    error: null,
    projects: [
      {
        id: PROJECT_ID,
        name: "Sage",
        rootCount: 1,
        lastOpenedAt: FIXED_NOW,
        railChats: [
          ...scenario.pages.map((pageValue, index) => ({
            id: pageValue.chatId,
            title: pageValue.title,
            pinned: index === 0,
            position: index,
            status:
              name === "error"
                ? ("failed" as const)
                : name === "waiting"
                  ? ("waiting" as const)
                  : index === 1
                    ? ("working" as const)
                    : ("complete" as const),
            updatedAt: FIXED_NOW - index * 60_000,
          })),
          {
            id: "visual-chat-attention",
            title: "Keyboard walkthrough needs attention",
            pinned: false,
            position: 8,
            status: "waiting",
            updatedAt: FIXED_NOW - 5 * 60_000,
          },
        ],
        railChatsTruncated: false,
      },
    ],
    project: {
      id: PROJECT_ID,
      name: "Sage",
      roots: [
        {
          id: ROOT_ID,
          name: "Sage",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
    tabs,
    archivedChats: [],
    activeTabId,
    selectedRootId: ROOT_ID,
    selectedResource: scenario.resource === "file" ? fileResource : null,
    sidebarOpen: true,
    sidebarView: "explorer",
    harnesses: [
      {
        id: "visual-acp",
        name: "Codex",
        description: "Deterministic ACP visual fixture",
        installState: "available",
        source: "development",
        accountModelDiscovery: false,
        transport: "acp",
        bindingId: BINDING_ID,
      },
    ],
    selectedHarnessId: "visual-acp",
    models: [{ id: MODEL_ID, name: "GPT-5" }],
    selectedModelId: MODEL_ID,
    harnessDiscoveryPhase: "ready",
    harnessDiscoveryError: null,
    sessionId: SESSION_ID,
    messages: [],
    chatUiById,
    chatError: null,
    composerText: "",
    sending: false,
    repositories: {
      roots: [
        {
          rootId: ROOT_ID,
          state: "repository",
          branch: "feature/editor-polish",
          upstream: null,
          ahead: 0,
          behind: 0,
          branches: ["feature/editor-polish", "main", "release/beta"],
          branchesTruncated: false,
          recentCommits: [
            { sha: "b7c12d66aa7de0f0a1", subject: "Polish editor tools" },
            { sha: "49a0ee42f7d08b93c2", subject: "Keep Git actions truthful" },
            {
              sha: "1e8c53a1c51f81f8aa",
              subject: "Align shared file surfaces",
            },
          ],
          recentCommitsTruncated: false,
          remotes: ["origin", "upstream"],
          remotesTruncated: false,
          changes: [
            {
              resource: fileResource,
              indexStatus: "unchanged",
              worktreeStatus: "modified",
              additions: 28,
              deletions: 11,
            },
          ],
          truncated: false,
          elapsedMs: 12,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 12,
    },
    turnChanges: [],
    directoryListings: {
      [resourceKey(rootListing.resource)]: {
        phase: "ready",
        value: rootListing,
      },
      [resourceKey(srcListing.resource)]: {
        phase: "ready",
        value: srcListing,
      },
      [resourceKey(workbenchListing.resource)]: {
        phase: "ready",
        value: workbenchListing,
      },
    },
    expandedDirectories: {
      [resourceKey(srcListing.resource)]: true,
      [resourceKey(workbenchListing.resource)]: true,
    },
    diffs: {
      [diffCacheKey(undefined, fileResource)]: {
        phase: "ready",
        value: visualDiff,
      },
    },
    activeEditor:
      scenario.resource === "file"
        ? {
            phase: "ready",
            resource: fileResource,
            document: editorDoc,
            content: editorContent,
            revision: 0,
            persistedRevision: 0,
            persisting: false,
            saving: false,
            message: null,
          }
        : null,
    activateTab: async (tabId: string) => {
      useWorkbenchStore.setState({ activeTabId: tabId });
    },
    loadRepositoryInventory: async () => undefined,
    loadDirectory: async () => undefined,
    loadDiff: async () => undefined,
  });

  const selectedOwner = chatIds[0]
    ? ({
        projectId: PROJECT_ID,
        chatId: chatIds[0],
        sessionId: SESSION_ID,
      } as AgentArtifactOwner)
    : null;
  if (
    scenario.mode === "agent" &&
    scenario.resource === "file" &&
    selectedOwner
  ) {
    useAgentArtifactAdmissionStore.getState().admit({
      owner: selectedOwner,
      tabId: "visual-file-tab",
    });
  }
  if (
    scenario.mode === "agent" &&
    scenario.resource === "diff" &&
    selectedOwner
  ) {
    useAgentArtifactAdmissionStore.getState().admit({
      owner: selectedOwner,
      tabId: "visual-diff-tab",
    });
  }
  setHarnessBrowseOwner(selectedOwner);
  if (scenario.resource === "preview" && selectedOwner) {
    applyBrowseIntent({
      owner: selectedOwner,
      url: `${window.location.origin}/tests/ui/fixtures/preview.html`,
    });
  }
  document.documentElement.dataset.visualScenario = name;
  await new Promise((resolve) =>
    requestAnimationFrame(() => resolve(undefined)),
  );
}

export async function installVisualExplorerScrollFixture() {
  const state = useWorkbenchStore.getState();
  const workbench = childResource(ROOT_ID, "src/workbench");
  const key = resourceKey(workbench);
  const listing = state.directoryListings[key];
  if (listing?.phase !== "ready") {
    throw new Error(
      "Visual Explorer scroll fixture requires workbench listing",
    );
  }
  const filler = Array.from({ length: 40 }, (_, index) => ({
    name: `fixture-${String(index).padStart(2, "0")}.ts`,
    resource: childResource(
      ROOT_ID,
      `src/workbench/fixture-${String(index).padStart(2, "0")}.ts`,
    ),
    kind: "file" as const,
    byteLen: 40 + index,
  }));
  useWorkbenchStore.setState({
    selectedResource: null,
    directoryListings: {
      ...state.directoryListings,
      [key]: {
        phase: "ready",
        value: {
          ...listing.value,
          entries: [...filler, ...listing.value.entries],
        },
      },
    },
  });
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  useWorkbenchStore.setState({ selectedResource: fileResource });
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}

export function installVisualExplorerActionFixture() {
  useWorkbenchStore.setState({
    selectedResource: childResource(ROOT_ID, "README.md"),
  });
}

export function installVisualExplorerUnavailableActionFixture() {
  useWorkbenchStore.setState({ selectedResource: null });
}

export async function installVisualAgentArtifactFollowFixture() {
  const state = useWorkbenchStore.getState();
  const chats = state.tabs.filter((tab) => tab.kind === "chat");
  const chatA = chats[0];
  const chatB = chats[1];
  if (!chatA || !chatB || state.project?.id !== PROJECT_ID) {
    throw new Error("Visual Agent artifact fixture requires two Chats");
  }
  const ownerA = {
    projectId: PROJECT_ID,
    chatId: chatA.id,
    sessionId: SESSION_ID,
  } as AgentArtifactOwner;
  const ownerB = {
    projectId: PROJECT_ID,
    chatId: chatB.id,
    sessionId: SESSION_ID,
  } as AgentArtifactOwner;
  const fileTab: WorkbenchTab = {
    id: "visual-follow-file-tab",
    kind: "file",
    title: "AgentChatPanes.tsx",
    position: chats.length,
    pinned: false,
    dirty: false,
    resource: fileResource,
  };
  const diffTab: WorkbenchTab = {
    ...fileTab,
    id: "visual-follow-diff-tab",
    kind: "diff",
    title: "AgentChatPanes.tsx changes",
    position: chats.length + 1,
  };
  useWorkbenchStore.setState({
    tabs: [...chats, fileTab, diffTab],
    activeTabId: fileTab.id,
    activeEditor: {
      phase: "ready",
      resource: fileResource,
      document: editorDocument(),
      content: editorContent,
      revision: 0,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
    },
  });
  useAgentArtifactAdmissionStore.getState().admit({
    owner: ownerA,
    tabId: diffTab.id,
  });
  useAgentArtifactFollowStore
    .getState()
    .pin(ownerA, { kind: "tab", tabId: diffTab.id });
  useAgentArtifactAdmissionStore.getState().admit({
    owner: ownerB,
    tabId: fileTab.id,
  });
  useAgentArtifactFollowStore
    .getState()
    .pin(ownerB, { kind: "tab", tabId: fileTab.id });
  applyBrowseIntent({
    owner: ownerB,
    url: `${window.location.origin}/tests/ui/fixtures/preview.html`,
  });
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}

export function installVisualRepositoryError() {
  useWorkbenchStore.setState({
    repositoryInventoryError: "Could not refresh repository inventory.",
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
  });
}

export function installVisualRepositoryLoading() {
  useWorkbenchStore.setState({
    repositoryInventoryError: null,
    repositoryStatusError: null,
    repositoryInventoryPhase: "loading",
    repositoryStatusPhase: "loading",
  });
}

export function installVisualRepositoryPrimaryState(
  primaryState: "ahead" | "behind" | "diverged",
) {
  const divergence = {
    ahead: { ahead: 2, behind: 0 },
    behind: { ahead: 0, behind: 1 },
    diverged: { ahead: 2, behind: 1 },
  }[primaryState];
  useWorkbenchStore.setState((state) => ({
    repositories: {
      ...state.repositories,
      roots: state.repositories.roots.map((repository) => ({
        ...repository,
        upstream: "origin/feature/editor-polish",
        changes: [],
        truncated: false,
        ...divergence,
      })),
    },
  }));
}

export function installVisualEmptyEditorTools() {
  languageDesktop.subscribeDiagnostics = async () => () => undefined;
  languageDesktop.subscribeOutline = async () => () => undefined;
}

export function installVisualEmptyEditorCanvas() {
  useWorkbenchStore.setState((state) => ({
    tabs: state.tabs.filter((tab) => tab.kind === "chat"),
    activeTabId: state.tabs.find((tab) => tab.kind === "chat")?.id ?? null,
    activeEditor: null,
    selectedResource: null,
  }));
}

export function installVisualDirtyEditor() {
  useWorkbenchStore.setState((state) => ({
    tabs: state.tabs.map((tab) =>
      tab.kind === "file" ? { ...tab, dirty: true } : tab,
    ),
    activeEditor:
      state.activeEditor?.phase === "ready"
        ? (() => {
            const content = `${state.activeEditor.content}\n// interaction-state review`;
            const revision = state.activeEditor.revision + 1;
            return {
              ...state.activeEditor,
              document: {
                ...state.activeEditor.document,
                buffer: {
                  ...state.activeEditor.document.buffer,
                  fingerprint: "visual-editor-dirty-v1",
                  byteLen: new TextEncoder().encode(content).byteLength,
                  revision,
                  content,
                },
                dirty: true,
              },
              content,
              revision,
              // This fixture captures the post-recovery-snapshot dirty state;
              // disk persistence still requires the explicit Save command.
              persistedRevision: revision,
            };
          })()
        : state.activeEditor,
  }));
}
