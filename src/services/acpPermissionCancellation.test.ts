import { describe, expect, it } from "vitest";

import { safeItem } from "./acpDesktopResponseItems";

describe("cancelled permission projection", () => {
  it("preserves offered option kinds for a waiting permission", () => {
    const item = safeItem({
      id: "item-offered",
      projectId: "project-one",
      chatId: "chat-one",
      sessionId: "session-one",
      turnId: "turn-one",
      activityId: null,
      runtimeEpoch: "1",
      revision: "1",
      ordinal: 1,
      origin: "primary",
      kind: "permissionRequest",
      lifecycle: "streaming",
      toolState: "waitingForApproval",
      createdAt: 1,
      updatedAt: 1,
      terminalAt: null,
      payload: {
        requestId: "request-one",
        state: "received",
        label: "Permission request",
        summary: "A command",
        offeredOptionKinds: ["reject_once"],
      },
    });
    expect(
      item?.kind === "permissionRequest" && item.payload.offeredOptionKinds,
    ).toEqual(["reject_once"]);
  });
  it("preserves browser permission source", () => {
    const item = safeItem({
      id: "item-browser",
      projectId: "project-one",
      chatId: "chat-one",
      sessionId: "session-one",
      turnId: "turn-one",
      activityId: null,
      runtimeEpoch: "1",
      revision: "1",
      ordinal: 1,
      origin: "primary",
      kind: "permissionRequest",
      lifecycle: "streaming",
      toolState: "waitingForApproval",
      createdAt: 1,
      updatedAt: 1,
      terminalAt: null,
      payload: {
        requestId: "request-one",
        state: "received",
        label: "Browser permission",
        summary: "Read page",
        source: "browser",
      },
    });
    expect(item?.kind === "permissionRequest" && item.payload.source).toBe(
      "browser",
    );
  });
  it("decodes truthful cancelled state without adding a presentation action", () => {
    const item = safeItem({
      id: "item-cancelled",
      projectId: "project-one",
      chatId: "chat-one",
      sessionId: "session-one",
      turnId: "turn-one",
      activityId: null,
      runtimeEpoch: "1",
      revision: "2",
      ordinal: 1,
      origin: "primary",
      kind: "permissionRequest",
      lifecycle: "completed",
      toolState: "cancelled",
      createdAt: 1,
      updatedAt: 2,
      terminalAt: 2,
      payload: {
        requestId: "request-one",
        state: "cancelled",
        label: "Permission request",
        summary: "Cancelled with the Turn",
      },
    });

    expect(item?.kind).toBe("permissionRequest");
    expect(item?.kind === "permissionRequest" && item.payload.state).toBe(
      "cancelled",
    );
  });
});
