import type {
  AcpChatPage,
  AcpItem,
  AgentSessionProjection,
  EntityRevision,
  ProjectionRevision,
  SageChatId,
  SageTurnId,
  TurnProjection,
  TurnState,
} from "../../../src/domain/acp";

import {
  FIXED_NOW,
  PROJECT_ID,
  RUNTIME_EPOCH,
  SESSION_ID,
  session,
} from "./visualFixtureIds";
import {
  errorItem,
  fileChangeItem,
  planItem,
  textItem,
  thoughtItem,
  toolItem,
} from "./visualItemBuilders";

export function turn(
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
    browserOperations: [],
    browserOperationsCollapsed: 0,
  };
}

export function turnItems(
  chatId: SageChatId,
  ordinal: number,
  options: { rich?: boolean; working?: boolean; failed?: boolean } = {},
) {
  const turnId = `visual-turn-${chatId}-${ordinal}` as SageTurnId;
  const createdAt = FIXED_NOW - (20 - ordinal) * 60_000;
  const input = (itemOrdinal: number) => ({
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

export function page(
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

export function standardPage(
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
