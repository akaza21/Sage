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

function fileChange(relativePath: string): object {
  return {
    resource: {
      kind: "resource",
      resource: { rootId: "root-a", relativePath },
    },
    operation: "updated",
    originalResource: null,
    summary: `Changed ${relativePath}`,
    additions: 1,
    deletions: 0,
  };
}

function item(index = 0, overrides: Record<string, unknown> = {}): object {
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
    ...overrides,
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

  it("fails closed when a plan payload exceeds its entry limit", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind: "plan",
                  payload: {
                    entries: Array.from({ length: 513 }, () => ({
                      label: "Plan step",
                      state: "pending",
                    })),
                  },
                }),
              ],
            }),
          ],
        }),
      ),
    ).toBeNull();
  });

  it("admits a file-change payload at the 64-change bound", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind: "fileChangeDiff",
                  payload: {
                    changes: Array.from({ length: 64 }, (_, index) =>
                      fileChange(`a${index}.ts`),
                    ),
                    truncated: false,
                  },
                }),
              ],
            }),
          ],
        }),
      ),
    ).not.toBeNull();
  });

  it("fails closed when a file-change payload exceeds its change limit", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind: "fileChangeDiff",
                  payload: {
                    changes: Array.from({ length: 65 }, (_, index) =>
                      fileChange(`a${index}.ts`),
                    ),
                    truncated: true,
                  },
                }),
              ],
            }),
          ],
        }),
      ),
    ).toBeNull();
  });

  it("fails closed when a tool presentation exceeds its resource limit", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind: "toolCall",
                  toolState: "completed",
                  payload: {
                    presentation: {
                      kind: "file",
                      label: "Read files",
                      resources: Array.from({ length: 65 }, () => ({
                        kind: "resource",
                        resource: {
                          rootId: "root-a",
                          relativePath: "a.ts",
                        },
                      })),
                    },
                    inputSummary: null,
                  },
                }),
              ],
            }),
          ],
        }),
      ),
    ).toBeNull();
  });

  it("fails closed when collaboration labels exceed their limit", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind: "collaboration",
                  payload: {
                    activityId: "activity-0",
                    label: "Delegated work",
                    summary: "A subagent completed work.",
                    receiverLabels: Array.from({ length: 33 }, () => "agent"),
                  },
                }),
              ],
            }),
          ],
        }),
      ),
    ).toBeNull();
  });

  it("admits a bounded png image item and rejects svg, oversize, and http urls", () => {
    const png =
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";
    const admitted = mutation({
      turns: [
        turn({
          items: [
            item(0, {
              kind: "image",
              payload: {
                mediaType: "image/png",
                dataUrl: `data:image/png;base64,${png}`,
              },
            }),
          ],
        }),
      ],
    });
    expect(safeAcpMutationResponse(admitted)).toEqual(admitted);

    for (const payload of [
      {
        mediaType: "image/svg+xml",
        dataUrl: "data:image/svg+xml;base64,PHN2Zy8+",
      },
      {
        mediaType: "image/png",
        dataUrl: `data:image/png;base64,${"A".repeat(262_144)}`,
      },
      {
        mediaType: "image/png",
        dataUrl: "https://example.com/diagram.png",
      },
    ]) {
      expect(
        safeAcpMutationResponse(
          mutation({
            turns: [
              turn({
                items: [item(0, { kind: "image", payload })],
              }),
            ],
          }),
        ),
      ).toBeNull();
    }
  });

  it("fails closed when an item payload discriminant mismatches its kind", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind: "text",
                  payload: { summary: "This is not text." },
                }),
              ],
            }),
          ],
        }),
      ),
    ).toBeNull();
  });
});
