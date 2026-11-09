import { renderHook } from "@testing-library/react";
import { expect, it } from "vitest";

import type {
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageProjectId,
} from "../../../domain/acp";
import { agentSessionFixture } from "../../../domain/acpProjection.fixture";
import type { AcpChatPage } from "../../../domain/acpProjection";
import { useUsageRequest } from "./useUsageRequest";

function page(usage: boolean): AcpChatPage {
  return {
    projectId: "project-a" as SageProjectId,
    chatId: "chat-a" as SageChatId,
    title: "Chat",
    projectionRevision: "1" as ProjectionRevision,
    currentSessionId: "session-a" as SageAgentSessionId,
    sessions: [
      agentSessionFixture({
        residency: "hot",
        runtimeEpoch: "2" as RuntimeEpoch,
      }),
    ],
    turns: [],
    nextCursor: null,
    ...(usage && { usage: true }),
  };
}

it("names the live runtime only when it reports usage limits", () => {
  expect(renderHook(() => useUsageRequest(page(false), 3)).result.current).toBe(
    null,
  );
  expect(
    renderHook(() => useUsageRequest(page(true), 3)).result.current,
  ).toEqual({
    projectId: "project-a",
    projectGeneration: "3",
    chatId: "chat-a",
    sessionId: "session-a",
    runtimeEpoch: "2",
  });
});
