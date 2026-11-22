import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
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
    throw { kind: "rejected", message: "Native refusal" };
  },
}));

afterEach(() => {
  cleanup();
  native.frames.length = 0;
  native.reply = null;
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

it.each([
  { replacement: "unseen", choice: "save" },
  { replacement: "none", choice: "save" },
  { replacement: "identity", choice: "Keep editing" },
  { replacement: "objective", choice: "Discard" },
])(
  "preserves native updates during an objective edit ($replacement, $choice)",
  async ({ replacement, choice }) => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const capabilities = {
      observes: true,
      set: true,
      pause: true,
      resume: true,
      tokenBudget: true,
      clear: true,
    };
    const snapshot: AcpGoalSnapshot = {
      objective: "Objective A",
      status: "active",
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
    const mutate = createGoalMutationController({
      getContext: () => context,
      set: goalMutations.setGoal,
      pause: goalMutations.pauseExistingGoal,
      clear: goalMutations.clearExistingGoal,
      apply: () => true,
      nextIntentId: () => "intent-a" as never,
    });
    const props = { capabilities, canMutate: true, onMutation: mutate };
    const view = render(
      <ChatGoalControl {...props} state={{ kind: "goal", snapshot }} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Edit goal" }));
    fireEvent.change(await screen.findByLabelText("Goal objective"), {
      target: { value: "My edited objective" },
    });
    const latest = {
      ...snapshot,
      tokenBudget: 3000,
      tokensUsed: 84,
      updatedAt: "2",
      ...(["objective", "unseen"].includes(replacement)
        ? { objective: "Objective B" }
        : {}),
      ...(replacement === "identity" ? { createdAt: "2" } : {}),
    };
    context.page = {
      ...context.page,
      projectionRevision: "2" as never,
      goal: { kind: "goal", snapshot: latest },
    };
    if (replacement !== "unseen")
      view.rerender(
        <ChatGoalControl
          {...props}
          state={{ kind: "goal", snapshot: latest }}
        />,
      );
    if (replacement !== "none" && replacement !== "unseen") {
      expect(screen.getByRole("alert")).toHaveTextContent(
        "The goal changed while you were editing",
      );
      expect(screen.getByRole("button", { name: "Save goal" })).toBeDisabled();
      fireEvent.click(screen.getByRole("button", { name: "Save goal" }));
      expect(native.frames).toEqual([]);
      fireEvent.click(screen.getByRole("button", { name: choice }));
      if (choice === "Discard") {
        expect(screen.queryByLabelText("Goal objective")).toBeNull();
        expect(native.frames).toEqual([]);
        return;
      }
    }
    fireEvent.click(screen.getByRole("button", { name: "Save goal" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Native refusal",
    );
    await waitFor(() =>
      expect(native.frames).toEqual([
        {
          command: "set_goal",
          request: {
            projectId: "project-a",
            projectGeneration: "0",
            chatId: "chat-a",
            expectedProjectionRevision: "2",
            clientIntentId: "intent-a",
            sessionId: "session-a",
            runtimeEpoch: "1",
            expectedGoal: {
              objective:
                replacement === "unseen"
                  ? snapshot.objective
                  : latest.objective,
              createdAt: latest.createdAt,
            },
            objective: "My edited objective",
          },
        },
      ]),
    );
    expect(screen.getByText("Pursuing goal")).toBeVisible();
  },
);

it("resumes a blocked native goal and renders the authoritative active state", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });
  const capabilities = {
    observes: true,
    set: true,
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
    scope: { projectId: "project-a", projectGeneration: "0", chatId: "chat-a" },
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
  fireEvent.click(screen.getByRole("button", { name: "Resume goal" }));
  expect(await screen.findByText("Pursuing goal")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Resume goal" })).toBeNull();
  expect(screen.getByRole("button", { name: "Pause goal" })).toBeEnabled();
  expect(native.frames).toEqual([
    {
      command: "resume_existing_goal",
      request: {
        projectId: "project-a",
        projectGeneration: "0",
        chatId: "chat-a",
        expectedProjectionRevision: "1",
        clientIntentId: "intent-resume",
        sessionId: "session-a",
        runtimeEpoch: "1",
      },
    },
  ]);
});
