import { expect } from "@playwright/test";

import type {
  CancelAcpTurnRequest,
  ClientIntentId,
  ProjectionRevision,
  SendAcpTurnRequest,
  TurnState,
} from "../../../src/domain/acp";
import type { audit } from "./agentTurnJourneyBrowser";
import {
  JOURNEY_IDS,
  type JourneyChat,
  type ProjectionAudit,
} from "./agentTurnJourneyModel";

type JourneyAudit = Awaited<ReturnType<typeof audit>>;

function expectIntentSequence(ids: readonly string[]): ClientIntentId[] {
  const prefix = "intent-ui-0001-";
  expect(ids[0]).toMatch(/^intent-ui-0001-.+/);
  const bootId = ids[0].slice(prefix.length);
  const expected = ids.map(
    (_, index) =>
      `intent-ui-${(index + 1).toString(36).padStart(4, "0")}-${bootId}` as ClientIntentId,
  );
  expect(ids).toEqual(expected);
  return expected;
}

function send(
  chat: Exclude<JourneyChat, "gamma">,
  prompt: string,
  expectedProjectionRevision: string,
  clientIntentId: ClientIntentId,
  projectGeneration = "7",
): SendAcpTurnRequest {
  return {
    projectId: JOURNEY_IDS.projectA,
    projectGeneration: projectGeneration as never,
    chatId: JOURNEY_IDS[chat],
    expectedProjectionRevision:
      expectedProjectionRevision as ProjectionRevision,
    clientIntentId,
    target: "session",
    sessionId: `journey-session-${chat}` as never,
    runtimeEpoch: ({ alpha: "11", beta: "22" } as const)[chat] as never,
    prompt,
  };
}

function accepted(
  chat: JourneyChat,
  projectionRevision: string,
  turnOrdinal: number,
  turnRevision: string,
  turnState: TurnState,
  projectGeneration = chat === "gamma" ? "8" : "7",
): ProjectionAudit {
  const projectId =
    chat === "gamma" ? JOURNEY_IDS.projectB : JOURNEY_IDS.projectA;
  return {
    projectId,
    projectGeneration,
    chatId: JOURNEY_IDS[chat],
    sessionId: `journey-session-${chat}`,
    sessionRevision: "1",
    turnId: `${JOURNEY_IDS[chat]}-turn-${turnOrdinal}`,
    turnRevision,
    turnState,
    runtimeEpoch: ({ alpha: "11", beta: "22", gamma: "33" } as const)[chat],
    projectionRevision,
    targetProjectId: projectId,
    targetChatId: JOURNEY_IDS[chat],
    disposition: "accepted",
  };
}

export function expectActionIdentityEvidence(result: JourneyAudit): void {
  const alphaIntents = expectIntentSequence([
    result.sendRequests[0].clientIntentId,
    result.cancelRequests[0].clientIntentId,
  ]);
  const betaIntents = expectIntentSequence([
    result.sendRequests[1].clientIntentId,
  ]);
  expect(result.sendRequests).toEqual([
    send("alpha", "Run Alpha once", "1", alphaIntents[0], "9"),
    send("beta", "Run Beta independently", "1", betaIntents[0], "10"),
  ]);
  expect(result.cancelRequests).toEqual([
    {
      projectId: JOURNEY_IDS.projectA,
      projectGeneration: "11",
      chatId: JOURNEY_IDS.alpha,
      expectedProjectionRevision: "3",
      clientIntentId: alphaIntents[1],
      sessionId: "journey-session-alpha",
      runtimeEpoch: "11",
      turnId: `${JOURNEY_IDS.alpha}-turn-1`,
      expectedTurnState: "running",
      expectedTurnRevision: "2",
    } satisfies CancelAcpTurnRequest,
  ]);
  expect(result.publications).toEqual([
    accepted("alpha", "2", 1, "1", "sending", "9"),
    accepted("alpha", "3", 1, "2", "running", "9"),
    accepted("beta", "2", 1, "1", "sending", "10"),
    accepted("beta", "3", 1, "2", "running", "10"),
    accepted("alpha", "4", 1, "3", "cancelling", "11"),
    accepted("alpha", "5", 1, "4", "cancelled", "11"),
  ]);
}

export function expectRecoveryIdentityEvidence(result: JourneyAudit): void {
  const intents = expectIntentSequence(
    result.sendRequests.map((request) => request.clientIntentId),
  );
  expect(result.sendRequests).toEqual([
    send("alpha", "Restore this exact prompt", "1", intents[0]),
    send("alpha", "Uncertain delivery prompt", "1", intents[1]),
  ]);
  expect(result.publications).toEqual([
    accepted("alpha", "2", 1, "1", "deliveryUnknown"),
  ]);
}

export function expectContinuityIdentityEvidence(result: JourneyAudit): void {
  const alphaIntents = expectIntentSequence(
    result.sendRequests.slice(0, 3).map((request) => request.clientIntentId),
  );
  const betaIntents = expectIntentSequence([
    result.sendRequests[3].clientIntentId,
  ]);
  expect(result.sendRequests).toEqual([
    send("alpha", "First terminal Turn", "1", alphaIntents[0]),
    send("alpha", "Explicit follow-up", "3", alphaIntents[1]),
    send("alpha", "Latest while scrolled away", "21", alphaIntents[2]),
    send("beta", "Background completion", "1", betaIntents[0], "8"),
  ]);
  expect(result.publications).toEqual([
    accepted("alpha", "2", 1, "1", "sending"),
    accepted("alpha", "3", 1, "2", "completed"),
    accepted("alpha", "4", 2, "1", "sending"),
    accepted("alpha", "5", 2, "2", "completed"),
    accepted("alpha", "21", 10, "2", "completed"),
    accepted("alpha", "22", 11, "1", "sending"),
    accepted("beta", "2", 1, "1", "sending", "8"),
    accepted("beta", "3", 1, "2", "running", "8"),
    accepted("beta", "4", 1, "3", "completed", "9"),
  ]);
}

export function expectStaleIdentityEvidence(result: JourneyAudit): void {
  const intents = expectIntentSequence([result.sendRequests[0].clientIntentId]);
  expect(result.sendRequests).toEqual([
    send("alpha", "Keep this Turn locked", "1", intents[0]),
  ]);
  expect(result.publications).toEqual([
    accepted("alpha", "2", 1, "1", "sending"),
    accepted("alpha", "3", 1, "2", "running"),
    {
      ...accepted("alpha", "3", 1, "2", "running", "9"),
      targetProjectId: JOURNEY_IDS.projectB,
      targetChatId: JOURNEY_IDS.gamma,
      disposition: "ignoredStaleIntent",
    },
  ]);
  expect(result.eventHistory).toEqual([
    "resolveSend:alpha",
    "publish:alpha",
    "stalePublication:alpha",
  ]);
}
