import { describe, expect, it } from "vitest";

import { safeAcpMutationResponse } from "./acpDesktopResponses";

function session(index = 0): object {
  return {
    id: `session-${index}`,
    ordinal: index,
    bindingId: "binding-a",
    modelId: null,
    state: "active",
    residency: "hot",
    runtimeEpoch: "1",
    nextRuntimeEpoch: null,
    capabilities: {
      schemaVersion: 1,
      protocolVersion: "1",
      newSession: true,
      resumeSession: true,
      loadSession: true,
      closeSession: true,
      prompt: true,
      cancel: true,
      nativeSessionFork: false,
      activeTurnSteer: false,
      childConversationProjection: false,
      childTranscriptRead: false,
      collaborationEventProjection: false,
      multiRootExtensionSchemaId: null,
      modelCount: 1,
      configCount: 0,
    },
    revision: "1",
    projectionRevision: "1",
    legacy: false,
  };
}

function item(index = 0): object {
  return {
    id: `item-${index}`,
    projectId: "project-a",
    chatId: "chat-a",
    sessionId: "session-0",
    turnId: "turn-0",
    runtimeEpoch: "1",
    ordinal: index,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: 1,
    toolState: null,
    kind: "text",
    payload: { role: "assistant", text: "done" },
  };
}

function activity(index = 0): object {
  return {
    id: `activity-${index}`,
    turnId: "turn-0",
    parentActivityId: null,
    parentItemId: "item-0",
    role: "root",
    lifecycle: "completed",
    depth: 0,
    provenance: {
      schemaVersion: 1,
      sourceKind: "tool",
      externalKind: "tool-a",
      extensionSchemaId: null,
      safeLabel: null,
    },
    revision: "1",
  };
}

function turn(overrides: Record<string, unknown> = {}): object {
  return {
    id: "turn-0",
    sessionId: "session-0",
    runtimeEpoch: "1",
    ordinal: 0,
    state: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: 1,
    items: [item()],
    activities: [activity()],
    ...overrides,
  };
}

function mutation(overrides: Record<string, unknown> = {}): object {
  return {
    projection: {
      projectId: "project-a",
      chatId: "chat-a",
      title: "Chat A",
      projectionRevision: "1",
      currentSessionId: "session-0",
      sessions: [session()],
      turns: [turn()],
      nextCursor: null,
      ...overrides,
    },
  };
}

describe("safeAcpMutationResponse", () => {
  it("returns a well-bounded mutation projection", () => {
    expect(safeAcpMutationResponse(mutation())).toEqual(mutation());
  });

  it("fails closed when sessions exceed the ACP page limit", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          sessions: Array.from({ length: 33 }, (_, index) => session(index)),
        }),
      ),
    ).toBeNull();
  });

  it("fails closed when turns exceed the ACP page limit", () => {
    expect(
      safeAcpMutationResponse(
        mutation({ turns: Array.from({ length: 21 }, () => turn()) }),
      ),
    ).toBeNull();
  });

  it("fails closed when page items or activities exceed ACP limits", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: Array.from({ length: 257 }, (_, index) => item(index)),
            }),
          ],
        }),
      ),
    ).toBeNull();
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              activities: Array.from({ length: 129 }, (_, index) =>
                activity(index),
              ),
            }),
          ],
        }),
      ),
    ).toBeNull();
  });

  it("fails closed on non-record sessions, turns, and items", () => {
    expect(safeAcpMutationResponse(mutation({ sessions: [null] }))).toBeNull();
    expect(safeAcpMutationResponse(mutation({ turns: [null] }))).toBeNull();
    expect(
      safeAcpMutationResponse(mutation({ turns: [turn({ items: [null] })] })),
    ).toBeNull();
  });
});
