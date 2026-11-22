import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { AcpGoalSnapshot } from "../../domain/acpGoal";
import type { AcpActionContext } from "./acp/acpActionController";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../domain/acpProjection.fixture";
import { goalMutations } from "../../services/acpGoalMutations";
import { createGoalMutationController } from "./acp/useGoalMutation";
import { ChatGoalControl } from "./ChatGoalControl";

const native = vi.hoisted(() => ({
  reply: null as unknown,
  frames: [] as { command: string; request: Record<string, unknown> }[],
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (
    command: string,
    { request }: { request: Record<string, unknown> },
  ) => {
    native.frames.push({ command, request });
    if (native.reply) return native.reply;
    throw {
      kind: "rejected",
      message: "The goal changed before Resume could start.",
    };
  },
}));

afterEach(() => {
  cleanup();
  native.frames.length = 0;
  native.reply = null;
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

it.each([false, true])(
  "resumes the displayed blocked goal, including a stale display: %s",
  async (stale) => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const capabilities = {
      observes: true,
      set: false,
      pause: true,
      resume: true,
      tokenBudget: false,
      clear: true,
    };
    const snapshot: AcpGoalSnapshot = {
      objective: "Ship",
      status: "blocked",
      tokenBudget: 1000,
      tokensUsed: 42,
      timeUsedSeconds: 10,
      createdAt: "1",
      updatedAt: "1",
    };
    const context = {
      scope: {
        projectId: "project-a",
        projectGeneration: "0",
        chatId: "chat-a",
      },
      ownershipEpoch: 0,
      commandProjectGeneration: "0",
      page: {
        projectId: "project-a",
        chatId: "chat-a",
        title: "Chat",
        projectionRevision: "1",
        currentSessionId: "session-a",
        turns: [],
        nextCursor: null,
        sessions: [
          agentSessionFixture({
            id: "session-a" as never,
            runtimeEpoch: "1" as never,
            capabilities: capabilitySetFixture({ goal: capabilities }),
          }),
        ],
        goal: { kind: "goal", snapshot },
      },
    } as unknown as AcpActionContext;
    native.reply = {
      projection: {
        ...context.page,
        projectionRevision: "2",
        goal: {
          kind: "goal",
          snapshot: { ...snapshot, status: "active", updatedAt: "2" },
        },
      },
    };
    function Goal() {
      const [page, setPage] = useState(context.page);
      const mutate = createGoalMutationController({
        getContext: () => context,
        resume: goalMutations.resumeExistingGoal,
        pause: goalMutations.pauseExistingGoal,
        clear: goalMutations.clearExistingGoal,
        apply: (_, result) => {
          context.page = result.projection;
          setPage(result.projection);
          return true;
        },
        nextIntentId: () => "intent-resume" as never,
      });
      return (
        <ChatGoalControl
          state={page.goal!}
          capabilities={capabilities}
          canMutate
          onMutation={mutate}
        />
      );
    }
    render(<Goal />);
    if (stale) {
      context.page = {
        ...context.page,
        projectionRevision: "2" as never,
        goal: { kind: "goal", snapshot: { ...snapshot, createdAt: "2" } },
      };
      native.reply = null;
    }
    fireEvent.click(screen.getByRole("button", { name: "Resume goal" }));
    if (stale) {
      expect(await screen.findByRole("alert")).toHaveTextContent(
        "goal changed",
      );
      expect(screen.getByText("Goal blocked")).toBeVisible();
    } else {
      expect(await screen.findByText("Pursuing goal")).toBeVisible();
      expect(screen.queryByRole("button", { name: "Resume goal" })).toBeNull();
      expect(screen.getByRole("button", { name: "Pause goal" })).toBeEnabled();
    }
    expect(native.frames).toEqual([
      {
        command: "resume_existing_goal",
        request: {
          projectId: "project-a",
          projectGeneration: "0",
          chatId: "chat-a",
          expectedProjectionRevision: stale ? "2" : "1",
          clientIntentId: "intent-resume",
          sessionId: "session-a",
          runtimeEpoch: "1",
          expectedGoal: { objective: "Ship", createdAt: "1" },
        },
      },
    ]);
  },
);
