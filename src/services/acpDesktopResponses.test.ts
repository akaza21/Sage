import { describe, expect, it } from "vitest";

import { decodeAcpGoal } from "../domain/acpGoal";
import { safeAcpMutationResponse } from "./acpDesktopResponses";
import { safeProjectionChanged } from "./acpDesktopProjection";

function session(index = 0, overrides: Record<string, unknown> = {}): object {
  return {
    id: `session-${index}`,
    ordinal: index,
    bindingId: "binding-a",
    modelId: null,
    state: "active",
    residency: "hot",
    runtimeEpoch: "1",
    nextRuntimeEpoch: null,
    permissionRequests: false,
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
    ...overrides,
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

function activity(index = 0, overrides: Record<string, unknown> = {}): object {
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
    ...overrides,
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
    browserOperations: [],
    browserOperationsCollapsed: 0,
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
      sessionConfig: null,
      turns: [turn()],
      nextCursor: null,
      ...overrides,
    },
  };
}

function mutationWithEntityRevisions(revision: unknown): object {
  return mutation({
    sessions: [session(0, { revision })],
    turns: [
      turn({
        revision,
        items: [item(0, { revision })],
        activities: [activity(0, { revision })],
      }),
    ],
  });
}

describe("safeAcpMutationResponse", () => {
  it.each([undefined, "attempt"])(
    "decodes the optional error kind: %s",
    (kind) => {
      const response = mutation({
        turns: [
          turn({
            items: [
              item(0, {
                kind: "error",
                lifecycle: "failed",
                payload: {
                  code: "Renamed label",
                  message: "Attempt cause",
                  recoverable: false,
                  ...(kind === undefined ? {} : { kind }),
                },
              }),
            ],
          }),
        ],
      });
      expect(safeAcpMutationResponse(response)).toEqual(response);
    },
  );

  it.each(["unknown", null, 1])(
    "degrades an unknown error kind: %s",
    (kind) => {
      const decoded = safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind: "error",
                  payload: {
                    code: "label",
                    message: "cause",
                    recoverable: false,
                    kind,
                  },
                }),
              ],
            }),
          ],
        }),
      );
      const error = decoded?.projection.turns[0]?.items[0];
      expect(error).toMatchObject({
        kind: "error",
        payload: { code: "label", message: "cause", recoverable: false },
      });
      expect(
        error?.kind === "error" ? error.payload.kind : "missing",
      ).toBeUndefined();
    },
  );
  it("accepts a bounded native Goal and isolates malformed Goal state", () => {
    const goal = {
      kind: "goal",
      snapshot: {
        objective: "Ship safely",
        status: "active",
        tokenBudget: 1000,
        tokensUsed: 42,
        timeUsedSeconds: 125,
        createdAt: null,
        updatedAt: null,
      },
    };
    expect(
      safeAcpMutationResponse(mutation({ goal }), decodeAcpGoal)?.projection
        .goal,
    ).toEqual(goal);
    expect(
      safeAcpMutationResponse(
        mutation({
          goal: { ...goal, snapshot: { ...goal.snapshot, objective: "" } },
        }),
        decodeAcpGoal,
      )?.projection.goal,
    ).toEqual({ kind: "unavailable" });
    expect(
      safeAcpMutationResponse(mutation({ goal }))?.projection.goal,
    ).toBeUndefined();
  });

  it("carries a bounded fork parent and rejects a malformed one", () => {
    expect(
      safeAcpMutationResponse(mutation({ forkedFrom: "chat-parent" }))
        ?.projection.forkedFrom,
    ).toBe("chat-parent");
    expect(safeAcpMutationResponse(mutation())?.projection).not.toHaveProperty(
      "forkedFrom",
    );
    for (const forkedFrom of ["", 7, null, "x".repeat(513)])
      expect(safeAcpMutationResponse(mutation({ forkedFrom }))).toBeNull();
  });

  it("carries the fork Turn and the child projection epoch, bounded", () => {
    const projection = safeAcpMutationResponse(
      mutation({
        forkedFrom: "chat-parent",
        forkedFromTurnId: "turn-fork",
        childProjectionEpoch: 1_792_798_848_091,
      }),
    )?.projection;
    expect(projection?.forkedFromTurnId).toBe("turn-fork");
    expect(projection?.childProjectionEpoch).toBe(1_792_798_848_091);
    expect(safeAcpMutationResponse(mutation())?.projection).not.toHaveProperty(
      "childProjectionEpoch",
    );
    // Both are display-only: an unsafe value is dropped, never trusted.
    for (const childProjectionEpoch of [-1, 0, 1.5, "7", 2 ** 60])
      expect(
        safeAcpMutationResponse(mutation({ childProjectionEpoch }))?.projection,
      ).not.toHaveProperty("childProjectionEpoch");
    for (const forkedFromTurnId of ["", 7, "x".repeat(513)])
      expect(
        safeAcpMutationResponse(mutation({ forkedFromTurnId }))?.projection,
      ).not.toHaveProperty("forkedFromTurnId");
  });

  it("carries the current session's surface and drops an unsafe one", () => {
    const sessionSurface = {
      sessionId: "session-0",
      title: "Fix the sidebar",
      notices: [{ level: "warning", text: "config deprecated" }],
      modelId: "rerouted",
      activities: [
        {
          key: "a".repeat(64),
          kind: "backgroundTask",
          label: "npm test",
          state: "running",
          progress: "12 of 40",
        },
      ],
      revision: "3",
    };
    expect(
      safeAcpMutationResponse(mutation({ sessionSurface }))?.projection
        .sessionSurface,
    ).toEqual(sessionSurface);
    const notice = { level: "info", text: "n" };
    for (const unsafe of [
      { ...sessionSurface, sessionId: "session-other" },
      { ...sessionSurface, notices: Array(17).fill(notice) },
      { ...sessionSurface, notices: [{ level: "debug", text: "n" }] },
      { ...sessionSurface, title: "x".repeat(257) },
      { ...sessionSurface, activities: [{ key: "task-7", kind: "k" }] },
    ])
      expect(
        safeAcpMutationResponse(mutation({ sessionSurface: unsafe }))
          ?.projection,
      ).not.toHaveProperty("sessionSurface");
  });

  it("accepts a child-epoch projection notice as an ordinary page wake-up", () => {
    const notice = {
      projectId: "project-0",
      chatId: "chat-0",
      sessionId: "session-0",
      projectionRevision: "4",
    };
    // Rust coalesces on the epoch; the WebView only reloads, and the page
    // it loads carries childProjectionEpoch.
    expect(
      safeProjectionChanged({ ...notice, childProjectionEpoch: 9 }),
    ).toEqual(notice);
  });

  it("returns a well-bounded mutation projection", () => {
    expect(safeAcpMutationResponse(mutation())).toEqual(mutation());
  });

  it("accepts an exact-owner session config removal tombstone with no advertised configs", () => {
    const projection = mutation({
      sessions: [session()],
      sessionConfig: {
        sageAgentSessionId: "session-0",
        runtimeEpoch: "1",
        configRevision: "2",
        removed: true,
        modes: null,
        configOptions: [],
      },
    });
    expect(safeAcpMutationResponse(projection)).toEqual(projection);
  });

  it("admits native zero entity revisions while retaining counter bounds", () => {
    expect(
      safeAcpMutationResponse(mutationWithEntityRevisions("0")),
    ).not.toBeNull();
    expect(
      safeAcpMutationResponse(
        mutationWithEntityRevisions("9223372036854775807"),
      ),
    ).not.toBeNull();

    for (const invalid of ["9223372036854775808", "01", "-1", 0]) {
      expect(
        safeAcpMutationResponse(mutationWithEntityRevisions(invalid)),
      ).toBeNull();
    }
  });

  it("admits a native-shaped fresh cold session at entity revision zero", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          sessions: [
            session(0, {
              state: "created",
              residency: "cold",
              runtimeEpoch: null,
              nextRuntimeEpoch: "1",
              revision: "0",
            }),
          ],
          turns: [],
        }),
      ),
    ).not.toBeNull();
  });

  it("admits a freshly created chat page at projection revision zero", () => {
    const freshChat = {
      projection: {
        sessionConfig: null,
        projectId: "45ae7ae4-fe47-44af-850f-59ba2ba59048",
        chatId: "eedef600-e483-403d-9b3d-0485c30cc5ae",
        title: "Chat: New chat 3",
        projectionRevision: "0",
        currentSessionId: null,
        sessions: [],
        turns: [],
        nextCursor: null,
      },
    };
    expect(safeAcpMutationResponse(freshChat)).toEqual(freshChat);
  });

  it("admits a zero page projection revision alongside zero entity revisions", () => {
    const zeroEntities = mutationWithEntityRevisions("0") as {
      projection: Record<string, unknown>;
    };
    expect(
      safeAcpMutationResponse({
        projection: { ...zeroEntities.projection, projectionRevision: "0" },
      }),
    ).not.toBeNull();
  });

  it("does not admit a zero runtime epoch with zero entity revisions", () => {
    const zeroEntities = mutationWithEntityRevisions("0") as {
      projection: Record<string, unknown>;
    };
    expect(
      safeAcpMutationResponse({
        projection: {
          ...zeroEntities.projection,
          sessions: [session(0, { revision: "0", runtimeEpoch: "0" })],
        },
      }),
    ).toBeNull();
  });

  it("keeps capabilities and identity relations fail-closed with zero entity revisions", () => {
    const zeroEntities = mutationWithEntityRevisions("0") as {
      projection: Record<string, unknown>;
    };
    expect(
      safeAcpMutationResponse({
        projection: { ...zeroEntities.projection, currentSessionId: "foreign" },
      }),
    ).toBeNull();
    expect(
      safeAcpMutationResponse({
        projection: {
          ...zeroEntities.projection,
          sessions: [
            session(0, {
              revision: "0",
              capabilities: {
                ...(session() as { capabilities: object }).capabilities,
                prompt: "true",
              },
            }),
          ],
        },
      }),
    ).toBeNull();
  });

  it("projects the session permission-request capability and rejects an absent value", () => {
    expect(
      safeAcpMutationResponse(mutation({ sessions: [session()] }))?.projection
        .sessions[0]?.permissionRequests,
    ).toBe(false);
    expect(
      safeAcpMutationResponse(
        mutation({ sessions: [session(0, { permissionRequests: undefined })] }),
      ),
    ).toBeNull();
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

  it("accepts a bounded compaction notice without tool state", () => {
    const response = safeAcpMutationResponse(
      mutation({
        turns: [
          turn({
            items: [
              item(0, {
                kind: "compactionNotice",
                toolState: null,
                payload: {},
              }),
            ],
          }),
        ],
      }),
    );
    expect(response?.projection.turns[0].items[0].kind).toBe(
      "compactionNotice",
    );
  });

  it("admits a file-change payload at the 32-change bound", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind: "fileChangeDiff",
                  payload: {
                    changes: Array.from({ length: 32 }, (_, index) =>
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

  it("preserves a safe relative label for a deleted missing file", () => {
    const response = safeAcpMutationResponse(
      mutation({
        turns: [
          turn({
            items: [
              item(0, {
                kind: "fileChangeDiff",
                payload: {
                  changes: [
                    {
                      ...fileChange("cp8c-probe.md"),
                      resource: {
                        kind: "unavailable",
                        reason: "missing",
                        label: "Path does not exist",
                        displayResource: {
                          rootId: "root-a",
                          relativePath: "cp8c-probe.md",
                        },
                      },
                      operation: "deleted",
                    },
                  ],
                  truncated: false,
                },
              }),
            ],
          }),
        ],
      }),
    );
    expect(response?.projection.turns[0]?.items[0]?.payload).toMatchObject({
      changes: [
        {
          resource: {
            kind: "unavailable",
            reason: "missing",
            displayResource: {
              rootId: "root-a",
              relativePath: "cp8c-probe.md",
            },
          },
        },
      ],
    });
  });

  it.each([
    "openTab",
    "navigate",
    "snapshotPage",
    "act",
    "waitFor",
    "screenshot",
    "readConsole",
    "readNetwork",
  ])("accepts the Rust browser operation %s", (operation) => {
    const response = mutation({
      turns: [
        turn({
          browserOperations: [
            {
              projectId: "project-a",
              chatId: "chat-a",
              sessionId: "session-0",
              turnId: "turn-0",
              tabId: "browser-1",
              operationId: "operation-1",
              operation,
              phase: "completed",
              origin: "https://example.com",
              summary: "Browser operation",
              resultSummary: "Completed",
              createdAt: 1,
              updatedAt: 1,
            },
          ],
        }),
      ],
    });
    expect(safeAcpMutationResponse(response)).not.toBeNull();
  });

  it("keeps known browser actions when another action has an unknown kind", () => {
    const operation = {
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-0",
      turnId: "turn-0",
      tabId: "browser-1",
      operationId: "operation-1",
      operation: "navigate",
      phase: "completed",
      origin: "https://example.com",
      summary: "Navigate to origin",
      resultSummary: "Completed",
      createdAt: 1,
      updatedAt: 1,
    };
    const decoded = safeAcpMutationResponse(
      mutation({
        turns: [
          turn({
            browserOperations: [
              operation,
              {
                ...operation,
                operationId: "operation-2",
                operation: "futureSecretAction",
                summary: "private operation input",
                resultSummary: "private operation output",
              },
            ],
          }),
        ],
      }),
    );
    expect(decoded?.projection.turns[0]?.browserOperations).toEqual([
      operation,
      {
        ...operation,
        operationId: "operation-2",
        operation: "unsupported",
        phase: "failed",
        summary: "Unsupported browser operation",
        resultSummary:
          "This browser operation is not supported by this version of Sage.",
      },
    ]);
  });

  it("rejects malformed browser actions even when their kind is unknown", () => {
    const base = {
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-0",
      turnId: "turn-0",
      tabId: "browser-1",
      operationId: "operation-1",
      operation: "futureAction",
      phase: "completed",
      origin: "https://example.com",
      summary: "Browser action",
      createdAt: 1,
      updatedAt: 1,
    };
    for (const malformed of [
      { ...base, operation: "x".repeat(129) },
      { ...base, turnId: "" },
      { ...base, summary: "x".repeat(513) },
      { ...base, phase: "mystery" },
    ]) {
      expect(
        safeAcpMutationResponse(
          mutation({ turns: [turn({ browserOperations: [malformed] })] }),
        ),
      ).toBeNull();
    }
  });

  it.each([
    "C:/secret.md",
    "../secret.md",
    "https:secret.md",
    "cp8\u0085probe.md",
  ])("rejects unsafe missing-file display path %s", (relativePath) => {
    const response = mutation({
      turns: [
        turn({
          items: [
            item(0, {
              kind: "fileChangeDiff",
              payload: {
                changes: [
                  {
                    ...fileChange("cp8c-probe.md"),
                    resource: {
                      kind: "unavailable",
                      reason: "missing",
                      label: "Path does not exist",
                      displayResource: { rootId: "root-a", relativePath },
                    },
                    operation: "deleted",
                  },
                ],
                truncated: false,
              },
            }),
          ],
        }),
      ],
    });
    expect(safeAcpMutationResponse(response)).toBeNull();
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
                    changes: Array.from({ length: 33 }, (_, index) =>
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
                      kind: "read",
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

  it("defaults legacy command activity to ordinary and rejects unknown activity", () => {
    const legacy = mutation({
      turns: [
        turn({
          items: [
            item(0, {
              kind: "toolCall",
              toolState: "completed",
              payload: {
                presentation: {
                  kind: "execute",
                  label: "Command",
                  summary: null,
                  resources: [],
                },
                inputSummary: null,
              },
            }),
          ],
        }),
      ],
    });
    expect(safeAcpMutationResponse(legacy)).not.toBeNull();

    const unknown = structuredClone(legacy) as {
      projection: {
        turns: {
          items: { payload: { presentation: { activity?: string } } }[];
        }[];
      };
    };
    unknown.projection.turns[0]!.items[0]!.payload.presentation.activity =
      "completedTests";
    expect(safeAcpMutationResponse(unknown)).toBeNull();
  });

  it("degrades an unknown tool kind to other instead of dropping the page", () => {
    const decoded = safeAcpMutationResponse(
      mutation({
        turns: [
          turn({
            items: [
              item(0, {
                kind: "toolCall",
                toolState: "completed",
                payload: {
                  presentation: {
                    kind: "teleport",
                    label: "Teleported",
                    summary: "raw input preserved",
                  },
                  inputSummary: null,
                },
              }),
            ],
          }),
        ],
      }),
    );
    const teleported = decoded?.projection.turns[0]?.items[0];
    expect(
      teleported?.kind === "toolCall" ? teleported.payload.presentation : null,
    ).toEqual({
      kind: "other",
      label: "Teleported",
      rawKind: "teleport",
      summary: "raw input preserved",
    });
  });

  it("admits exactly 64 command locations and rejects one more", () => {
    const command = (count: number) =>
      mutation({
        turns: [
          turn({
            items: [
              item(0, {
                kind: "toolCall",
                toolState: "completed",
                payload: {
                  inputSummary: null,
                  outputSummary: "bounded output",
                  presentation: {
                    kind: "execute",
                    label: "Command",
                    summary: null,
                    resources: Array.from({ length: count }, (_, index) => ({
                      kind: "resource",
                      resource: {
                        rootId: "root-a",
                        relativePath: `file-${index}.ts`,
                      },
                    })),
                  },
                },
              }),
            ],
          }),
        ],
      });
    expect(safeAcpMutationResponse(command(64))).not.toBeNull();
    expect(safeAcpMutationResponse(command(65))).toBeNull();
  });

  it("rejects a folded toolUpdateResult kind with no legacy decode path", () => {
    const legacy = mutation({
      turns: [
        turn({
          items: [
            item(0, {
              kind: "toolUpdateResult",
              toolState: "completed",
              payload: {
                callItemId: null,
                outputSummary: "folded away by migration 011",
                presentation: {
                  kind: "execute",
                  label: "Command",
                  summary: null,
                  resources: [],
                },
              },
            }),
          ],
        }),
      ],
    });

    expect(safeAcpMutationResponse(legacy)).toBeNull();
  });

  it.each([["toolCall", "outputSummary"]] as const)(
    "bounds %s command output by UTF-8 bytes",
    (kind, outputKey) => {
      const output = "😀".repeat(16_384);
      const command = (value: string) =>
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind,
                  toolState: "completed",
                  payload: {
                    presentation: {
                      kind: "execute",
                      label: "Command",
                      summary: null,
                      resources: [],
                    },
                    inputSummary: null,
                    [outputKey]: value,
                  },
                }),
              ],
            }),
          ],
        });
      expect(new TextEncoder().encode(output).byteLength).toBe(65_536);
      expect(safeAcpMutationResponse(command(output))).not.toBeNull();
      expect(safeAcpMutationResponse(command(`${output}x`))).toBeNull();
    },
  );

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

  it("loads a root collaboration item without an activity", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  kind: "collaboration",
                  payload: {
                    activityId: null,
                    label: "Subagent",
                    summary: "",
                    receiverLabels: [],
                    operation: "started",
                    agentKey: "child-1",
                    agentKeys: [],
                    agentStates: [],
                  },
                }),
              ],
            }),
          ],
        }),
      ),
    ).not.toBeNull();
  });

  it("rejects a harness activity collaboration item without its activity", () => {
    expect(
      safeAcpMutationResponse(
        mutation({
          turns: [
            turn({
              items: [
                item(0, {
                  origin: "harnessActivity",
                  kind: "collaboration",
                  payload: {
                    activityId: null,
                    label: "Subagent",
                    summary: "",
                    receiverLabels: [],
                    operation: "started",
                    agentKey: "child-1",
                    agentKeys: [],
                    agentStates: [],
                  },
                }),
              ],
            }),
          ],
        }),
      ),
    ).toBeNull();
  });

  it("rejects legacy usage transcript items", () => {
    const usageMutation = (contextWindow: unknown, include = true) =>
      mutation({
        turns: [
          turn({
            items: [
              item(0, {
                kind: "usage",
                payload: {
                  inputTokens: 4_000,
                  outputTokens: 2_000,
                  cachedInputTokens: null,
                  totalTokens: 6_000,
                  ...(include ? { contextWindow } : {}),
                },
              }),
            ],
          }),
        ],
      });

    expect(safeAcpMutationResponse(usageMutation(272_000))).toBeNull();
    expect(safeAcpMutationResponse(usageMutation(undefined, false))).toBeNull();
  });

  it.each([
    0,
    -1,
    1.5,
    0x1_0000_0000,
    Number.MAX_SAFE_INTEGER + 1,
    "272000",
    null,
  ])(
    "fails closed on invalid optional usage context window %s",
    (contextWindow) => {
      expect(
        safeAcpMutationResponse(
          mutation({
            turns: [
              turn({
                items: [
                  item(0, {
                    kind: "usage",
                    payload: {
                      inputTokens: 4_000,
                      outputTokens: 2_000,
                      cachedInputTokens: null,
                      totalTokens: 6_000,
                      contextWindow,
                    },
                  }),
                ],
              }),
            ],
          }),
        ),
      ).toBeNull();
    },
  );

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
