import type {
  AcpChatPage,
  AcpItem,
  EntityRevision,
  HarnessActivityProjection,
  SageChatId,
  SageTurnId,
} from "../../../../src/domain/acp";

import { FIXED_NOW, ROOT_ID } from "../visualFixtureIds";
import type { ItemInput } from "../visualFixtureIds";
import { itemIdentity } from "../visualFixtureIds";
import {
  errorItem,
  planItem,
  textItem,
  thoughtItem,
} from "../visualItemBuilders";
import { page, standardPage, turn, turnItems } from "../visualChatPages";
import type { VisualScenarioName } from "../visualScenarioTypes";

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

type ToolCallItem = Extract<AcpItem, { kind: "toolCall" }>;

function activityToolItem(
  input: ItemInput,
  presentation: ToolCallItem["payload"]["presentation"],
  state: ToolCallItem["toolState"] = "completed",
  outputSummary?: string,
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
      ...(outputSummary ? { outputSummary } : {}),
    },
  } as ToolCallItem;
}

function composerContextPage(
  chatIdValue: string,
  title: string,
  totalTokens: number,
  contextWindow?: number,
) {
  const chatId = chatIdValue as SageChatId;
  const turnId = `visual-turn-${chatId}-1` as SageTurnId;
  const createdAt = FIXED_NOW - 60_000;
  const input = (ordinal: number): ItemInput => ({
    chatId,
    turnId,
    ordinal,
    createdAt: createdAt + ordinal * 1_000,
  });
  const reportedTurn = turn(chatId, 1, "completed", [
    textItem(
      input(1),
      "assistant",
      "The runtime reported the current composer context usage.",
    ),
  ]);
  return page(chatId, title, [
    {
      ...reportedTurn,
      usage: {
        inputTokens: totalTokens,
        outputTokens: 0,
        totalTokens,
        ...(contextWindow === undefined ? {} : { contextWindow }),
      },
    },
  ]);
}

function sessionModePage(chatIdValue: string, title: string) {
  const result = standardPage(chatIdValue, title, "rich");
  return {
    ...result,
    sessionConfig: {
      sageAgentSessionId: "visual-session",
      runtimeEpoch: "7",
      configRevision: "1",
      modes: {
        currentModeId: "review",
        availableModes: [
          {
            id: "review",
            name: "Review changes",
            description: "Analyze changes without applying them.",
          },
          {
            id: "apply",
            name: "Apply changes",
            description: "Apply approved changes to the workspace.",
          },
        ],
      },
      configOptions: [],
    },
  };
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
    activityToolItem(
      input(3),
      {
        kind: "execute",
        label: "Run focused checks",
        summary:
          "npm test -- src/features/chat/acp/ToolItem.test.tsx src/features/chat/acp/TurnActivityItem.test.tsx src/features/chat/acp/AcpChatProjection.test.tsx",
        cwd: null,
        exitCode: 0,
        outputTail: null,
        outputTruncated: false,
        resources: [],
        activity: "ordinary",
      },
      "completed",
      "Focused checks passed for ToolItem, TurnActivityItem, and AcpChatProjection. This deliberately bounded fixture output records the completed command result across parser coverage, mounted runtime lifecycle, visual projection ownership, accessibility labels, and compact-width containment so the activity card exercises the existing long-output overflow contract without using presentation text, input text, or raw process output.",
    ),
    activityToolItem(
      input(4),
      {
        kind: "search",
        label: "Search workspace",
        querySummary: "activity card ownership",
        resources: [
          {
            kind: "resource",
            resource: {
              rootId: ROOT_ID,
              relativePath: "src/features/chat/acp/ToolItem.tsx",
            },
          },
        ],
        matchCount: 14,
        truncated: false,
      },
      "inProgress",
    ),
    activityToolItem(input(5), {
      kind: "read",
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
      kind: "fetch",
      label: "Review preview",
      url: "http://127.0.0.1:1431/",
      summary: "Opened http://127.0.0.1:1431/",
    }),
    activityToolItem(input(7), {
      kind: "other",
      label: "Inspect extension state",
      rawKind: "harness.extension_state",
      summary: "Normalized bounded activity",
    }),
    errorItem(input(8)),
    unsupportedItem(input(9)),
  ];
  return page(
    chatId,
    "Activity card polish",
    [turn(chatId, 1, "running", items)],
    "active",
  );
}

function declaredTestPage() {
  const chatId = "visual-chat-declared-test" as SageChatId;
  const turnId = `visual-turn-${chatId}-1` as SageTurnId;
  const input = (ordinal: number): ItemInput => ({
    chatId,
    turnId,
    ordinal,
    createdAt: FIXED_NOW - 60_000 + ordinal * 1_000,
  });
  return page(chatId, "Declared test run", [
    turn(chatId, 1, "completed", [
      textItem(
        input(1),
        "assistant",
        "I ran the project's declared test script.",
      ),
      activityToolItem(input(2), {
        kind: "execute",
        label: "Command",
        summary: null,
        cwd: null,
        exitCode: 0,
        outputTail: null,
        outputTruncated: false,
        resources: [],
        activity: "declaredTest",
      }),
    ]),
  ]);
}

function liveThinkingPage() {
  const chatId = "visual-chat-thinking-live" as SageChatId;
  const thought = thoughtItem(
    {
      chatId,
      turnId: `visual-turn-${chatId}-1` as SageTurnId,
      ordinal: 1,
      createdAt: FIXED_NOW - 1_000,
    },
    `**Inspecting**\n**Reviewing** ${"updated workspace state ".repeat(20)}\n\n`,
  );
  thought.lifecycle = "streaming";
  thought.terminalAt = null;
  return page(
    chatId,
    "Live thinking",
    [turn(chatId, 1, "running", [thought])],
    "active",
  );
}

function commandRollupPage() {
  const chatId = "visual-chat-command-rollup" as SageChatId;
  const turnId = `visual-turn-${chatId}-1` as SageTurnId;
  const input = (ordinal: number): ItemInput => ({
    chatId,
    turnId,
    ordinal,
    createdAt: FIXED_NOW - 60_000 + ordinal * 1_000,
  });
  const command = (ordinal: number, summary: string) =>
    activityToolItem(input(ordinal), {
      kind: "execute",
      label: "Command",
      summary,
      cwd: null,
      exitCode: 0,
      outputTail: null,
      outputTruncated: false,
      resources: [],
      activity: "ordinary",
    });
  return page(chatId, "Command rollup", [
    turn(chatId, 1, "completed", [
      textItem(
        input(1),
        "assistant",
        "I read alpha.txt followed by README.md, one read-only call per file.",
      ),
      command(
        2,
        "powershell -NoProfile -Command 'Get-Content -Path alpha.txt -TotalCount 40'",
      ),
      command(
        3,
        "powershell -NoProfile -Command 'Get-Content -Path README.md -TotalCount 40'",
      ),
    ]),
  ]);
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

export function buildAgentPages(
  name: VisualScenarioName,
): AcpChatPage[] | null {
  switch (name) {
    case "agent-single":
      return [
        standardPage(
          "visual-chat-agent-single",
          "Agent navigation polish",
          "rich",
        ),
      ];
    case "agent-dual":
      return [
        standardPage("visual-chat-primary", "Agent navigation polish", "rich"),
        standardPage(
          "visual-chat-secondary",
          "Editor consistency pass",
          "waiting",
        ),
      ];
    case "agent-long":
      return [longPage()];
    case "agent-activity-family":
      return [activityFamilyPage()];
    case "agent-thinking-live":
      return [liveThinkingPage()];
    case "agent-declared-test":
      return [declaredTestPage()];
    case "agent-command-rollup":
      return [commandRollupPage()];
    case "agent-image":
      return [
        imagePage(),
        standardPage(
          "visual-chat-image-review",
          "Check image result",
          "waiting",
        ),
      ];
    case "agent-child-activity":
      return [childActivityPage()];
    case "agent-picker-harness-model":
      return [standardPage("visual-chat-picker", "Picker readiness", "rich")];
    case "agent-picker-handoff":
      return [
        standardPage("visual-chat-handoff-source", "Source chat", "rich"),
        standardPage("visual-chat-handoff-target", "Target chat", "waiting"),
      ];
    case "agent-composer-narrow":
      return [
        standardPage("visual-chat-composer-narrow", "Narrow composer", "rich"),
      ];
    case "agent-composer-context-unknown":
      return [
        composerContextPage(
          "visual-chat-composer-context-unknown",
          "Composer context unknown capacity",
          6_000,
        ),
      ];
    case "agent-composer-context-known":
      return [
        composerContextPage(
          "visual-chat-composer-context-known",
          "Composer context known capacity",
          6_000,
          272_000,
        ),
      ];
    case "agent-composer-goal":
      return [
        {
          ...composerContextPage(
            "visual-chat-composer-goal",
            "Composer goal and context",
            6_000,
            272_000,
          ),
          goal: {
            kind: "goal",
            snapshot: {
              objective: "Ship the release safely",
              status: "active",
              tokenBudget: 1_000,
              tokensUsed: 42,
              timeUsedSeconds: 125,
              createdAt: null,
              updatedAt: null,
            },
          },
        },
      ];
    case "agent-composer-context-over-capacity":
      return [
        composerContextPage(
          "visual-chat-composer-context-over-capacity",
          "Composer context over capacity",
          300_000,
          272_000,
        ),
      ];
    case "agent-session-mode-closed":
      return [
        sessionModePage("visual-chat-session-mode-closed", "Session mode"),
      ];
    case "agent-session-mode-open":
      return [sessionModePage("visual-chat-session-mode-open", "Session mode")];
    case "agent-session-mode-narrow":
      return [
        sessionModePage("visual-chat-session-mode-narrow", "Session mode"),
      ];
    case "agent-session-mode-forced-colors":
      return [
        sessionModePage(
          "visual-chat-session-mode-forced-colors",
          "Session mode",
        ),
      ];
    default:
      return null;
  }
}
