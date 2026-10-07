import { describe, expect, it } from "vitest";

import type { AcpChatPage, AgentSessionProjection } from "../../../domain/acp";
import { agentSessionFixture } from "../../../domain/acpProjection.fixture";
import { permissionRequestsAvailable } from "./acpTurnState";

function page(
  currentSessionId: string | null,
  sessions: readonly AgentSessionProjection[],
): AcpChatPage {
  return {
    projectId: "project-a",
    chatId: "chat-a",
    title: "Permission capability",
    projectionRevision: "1",
    currentSessionId,
    sessions,
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

const supported = agentSessionFixture({ permissionRequests: true });
const unsupported = agentSessionFixture({ permissionRequests: false });

describe("permission request capability", () => {
  it("reports the current session projection", () => {
    expect(permissionRequestsAvailable(page("session-a", [supported]))).toBe(
      true,
    );
    expect(permissionRequestsAvailable(page("session-a", [unsupported]))).toBe(
      false,
    );
  });

  it("fails closed without a current session", () => {
    expect(permissionRequestsAvailable(null)).toBe(false);
    expect(permissionRequestsAvailable(page(null, [supported]))).toBe(false);
    expect(permissionRequestsAvailable(page("session-b", [supported]))).toBe(
      false,
    );
  });
});
