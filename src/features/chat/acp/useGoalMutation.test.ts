import { expect, it, vi } from "vitest";
import type { AcpActionContext } from "./acpActionController";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../../domain/acpProjection.fixture";
import { AcpDesktopError } from "../../../services/acpDesktop";
import { canMutateGoal, createGoalMutationController } from "./useGoalMutation";

function context(runtimeEpoch: string | null = "9"): AcpActionContext {
  return {
    scope: { projectId: "project-1", projectGeneration: "3", chatId: "chat-1" },
    ownershipEpoch: 2,
    commandProjectGeneration: "5",
    page: {
      projectId: "project-1",
      chatId: "chat-1",
      title: "A",
      projectionRevision: "8",
      currentSessionId: "session-1",
      nextCursor: null,
      turns: [],
      sessions: [
        agentSessionFixture({
          id: "session-1" as never,
          runtimeEpoch: runtimeEpoch as never,
          capabilities: capabilitySetFixture({
            protocolVersion: "codex-app-server-observed-0.167.0",
          }),
        }),
      ],
      goal: {
        kind: "goal",
        snapshot: {
          objective: "Ship",
          status: "active",
          tokenBudget: null,
          tokensUsed: 0,
          timeUsedSeconds: 2,
          createdAt: null,
          updatedAt: null,
        },
      },
    },
  } as unknown as AcpActionContext;
}

it("allows native Goal mutation only for a live session's current Goal", () => {
  expect(canMutateGoal(context().page)).toBe(true);
  expect(canMutateGoal(context(null).page)).toBe(false);
  for (const goal of [
    { kind: "noGoal" },
    { kind: "unsupported" },
    { kind: "offline" },
  ] as const) {
    expect(canMutateGoal({ ...context().page, goal })).toBe(false);
  }
});

it("sends the current scoped revision and applies only the authoritative response", async () => {
  const captured = context();
  const pause = vi.fn(async () => ({
    projection: { ...captured.page, projectionRevision: "9" as never },
  }));
  const apply = vi.fn(() => true);
  const mutate = createGoalMutationController({
    getContext: () => captured,
    pause,
    clear: vi.fn(),
    apply,
    nextIntentId: () => "intent-1" as never,
  });
  expect(await mutate("pause")).toEqual({ kind: "applied" });
  expect(pause).toHaveBeenCalledWith({
    projectId: "project-1",
    projectGeneration: "5",
    chatId: "chat-1",
    expectedProjectionRevision: "8",
    clientIntentId: "intent-1",
    sessionId: "session-1",
    runtimeEpoch: "9",
  });
  expect(apply).toHaveBeenCalledWith(captured, {
    projection: expect.objectContaining({ projectionRevision: "9" }),
  });
});

it("separates a refusal from an unknown outcome and locks unknown retries", async () => {
  const captured = context();
  const clear = vi
    .fn()
    .mockRejectedValueOnce(
      new AcpDesktopError({ kind: "rejected", message: "No." }),
    )
    .mockRejectedValueOnce(
      new AcpDesktopError({ kind: "recoveryRequired", message: "Check." }),
    );
  const mutate = createGoalMutationController({
    getContext: () => captured,
    pause: vi.fn(),
    clear,
    apply: vi.fn(),
    nextIntentId: () => "intent-1" as never,
  });
  expect((await mutate("clear")).kind).toBe("refused");
  expect((await mutate("clear")).kind).toBe("outcomeUnknown");
  expect((await mutate("clear")).kind).toBe("refused");
  expect(clear).toHaveBeenCalledTimes(2);
});

it.each([
  { scope: { projectId: "other-project" } },
  { scope: { projectGeneration: "4" } },
  { scope: { chatId: "other-chat" } },
  { ownershipEpoch: 3 },
  { commandProjectGeneration: "6" },
])(
  "refuses a Goal mutation after its captured owner changes: %j",
  async (change) => {
    const captured = context();
    const changed = {
      ...captured,
      ...change,
      scope: { ...captured.scope, ...change.scope },
    } as AcpActionContext;
    const pause = vi.fn();
    const mutate = createGoalMutationController({
      getContext: vi
        .fn()
        .mockReturnValueOnce(captured)
        .mockReturnValue(changed),
      pause,
      clear: vi.fn(),
      apply: vi.fn(),
      nextIntentId: () => "intent-1" as never,
    });
    expect(await mutate("pause")).toEqual({
      kind: "refused",
      message: "Goal session changed before the action could start.",
    });
    expect(pause).not.toHaveBeenCalled();
  },
);
