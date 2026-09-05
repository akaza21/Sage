import { describe, expect, it } from "vitest";

import { safeItem } from "./acpDesktopResponseItems";

describe("cancelled permission projection", () => {
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
