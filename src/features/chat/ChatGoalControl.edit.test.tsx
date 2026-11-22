import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
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
  frames: [] as { command: string; request: Record<string, unknown> }[],
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (
    command: string,
    { request }: { request: Record<string, unknown> },
  ) => {
    native.frames.push({ command, request });
    throw { kind: "rejected", message: "Native refusal" };
  },
}));

afterEach(() => {
  cleanup();
  native.frames.length = 0;
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

it.each([
  { field: "budget", replacement: "none", choice: "save" },
  { field: "objective", replacement: "none", choice: "save" },
  { field: "budget", replacement: "objective", choice: "Keep editing" },
  { field: "objective", replacement: "identity", choice: "Keep editing" },
  { field: "budget", replacement: "identity", choice: "Discard" },
  { field: "objective", replacement: "objective", choice: "Discard" },
])(
  "preserves native updates during a $field edit ($replacement, $choice)",
  async ({ field, replacement, choice }) => {
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
      tokenBudget: goalMutations.setGoalBudget,
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
    if (field === "budget") {
      const input = await screen.findByLabelText("Token budget (optional)");
      for (const value of ["0", "-1", "1.5", "9007199254740992"]) {
        fireEvent.change(input, { target: { value } });
        expect(
          screen.getByRole("button", { name: "Save goal" }),
        ).toBeDisabled();
      }
    }
    fireEvent.change(
      await screen.findByLabelText(
        field === "budget" ? "Token budget (optional)" : "Goal objective",
      ),
      {
        target: { value: field === "budget" ? "2000" : "My edited objective" },
      },
    );
    const latest = {
      ...snapshot,
      tokenBudget: 3000,
      tokensUsed: 84,
      updatedAt: "2",
      ...(replacement === "objective" ? { objective: "Objective B" } : {}),
      ...(replacement === "identity" ? { createdAt: "2" } : {}),
    };
    context.page = {
      ...context.page,
      projectionRevision: "2" as never,
      goal: { kind: "goal", snapshot: latest },
    };
    view.rerender(
      <ChatGoalControl {...props} state={{ kind: "goal", snapshot: latest }} />,
    );
    if (replacement !== "none") {
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
          command: field === "budget" ? "set_goal_budget" : "set_goal",
          request: {
            projectId: "project-a",
            projectGeneration: "0",
            chatId: "chat-a",
            expectedProjectionRevision: "2",
            clientIntentId: "intent-a",
            sessionId: "session-a",
            runtimeEpoch: "1",
            ...(field === "budget"
              ? { tokenBudget: 2000 }
              : { objective: "My edited objective" }),
          },
        },
      ]),
    );
    expect(screen.getByText("Pursuing goal")).toBeVisible();
  },
);
