import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { AcpGoalSnapshot } from "../../domain/acpGoal";
import { ChatGoalControl } from "./ChatGoalControl";

afterEach(cleanup);

it("shows an active native Goal above the composer and exposes details", async () => {
  render(
    <ChatGoalControl
      state={{
        kind: "goal",
        snapshot: {
          objective: "Ship the release safely",
          status: "active",
          tokenBudget: 1000,
          tokensUsed: 42,
          timeUsedSeconds: 125,
          createdAt: null,
          updatedAt: null,
        },
      }}
    />,
  );
  expect(screen.getByText("Pursuing goal")).toBeVisible();
  expect(screen.getAllByText("Ship the release safely")[0]).toBeVisible();
  expect(screen.getByText("2m 5s")).toBeVisible();
  expect(screen.queryByText("42 / 1,000 tokens")).toBeNull();
  screen.getByText("Pursuing goal").click();
  expect(await screen.findByText("42 / 1,000 tokens")).toBeVisible();
});

it("explains a pinned binding's Goal unavailability at the entry point", () => {
  render(<ChatGoalControl state={{ kind: "unsupported" }} />);
  screen.getByText("Goal").click();
  expect(screen.getByText(/not available for this binding/i)).toBeVisible();
  expect(screen.queryByText("Pursuing goal")).toBeNull();
});

it("offers pause and delete only for a binding with native Goal mutation", async () => {
  const onMutation = vi.fn(async () => ({
    kind: "refused" as const,
    message: "Harness refused pause.",
  }));
  render(
    <ChatGoalControl
      state={{
        kind: "goal",
        snapshot: {
          objective: "Ship",
          status: "active",
          tokenBudget: null,
          tokensUsed: 0,
          timeUsedSeconds: 3,
          createdAt: null,
          updatedAt: null,
        },
      }}
      capabilities={{
        observes: true,
        set: false,
        pause: true,
        resume: false,
        tokenBudget: false,
        clear: true,
      }}
      canMutate
      onMutation={onMutation}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Pause goal" }));
  await waitFor(() => expect(onMutation).toHaveBeenCalledWith("pause"));
  expect(screen.getByText("Harness refused pause.")).toBeVisible();
  expect(screen.getByText("Pursuing goal")).toBeVisible();
  expect(screen.getByRole("button", { name: "Delete goal" })).toBeEnabled();
});

it("keeps the observed Goal visible and locks controls when a delete outcome is unknown", async () => {
  const onMutation = vi.fn(async () => ({
    kind: "outcomeUnknown" as const,
    message: "Reload to verify.",
  }));
  render(
    <ChatGoalControl
      state={{
        kind: "goal",
        snapshot: {
          objective: "Ship",
          status: "active",
          tokenBudget: null,
          tokensUsed: 0,
          timeUsedSeconds: 3,
          createdAt: null,
          updatedAt: null,
        },
      }}
      capabilities={{
        observes: true,
        set: false,
        pause: true,
        resume: false,
        tokenBudget: false,
        clear: true,
      }}
      canMutate
      onMutation={onMutation}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete goal" }));
  await waitFor(() =>
    expect(screen.getByText("Reload to verify.")).toBeVisible(),
  );
  expect(screen.getByText("Pursuing goal")).toBeVisible();
  expect(screen.getByRole("button", { name: "Pause goal" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Delete goal" })).toBeDisabled();
});

it("does not offer native mutation buttons without an advertised mutation capability", () => {
  render(
    <ChatGoalControl
      state={{
        kind: "goal",
        snapshot: {
          objective: "Ship",
          status: "active",
          tokenBudget: null,
          tokensUsed: 0,
          timeUsedSeconds: 3,
          createdAt: null,
          updatedAt: null,
        },
      }}
    />,
  );
  expect(screen.queryByRole("button", { name: "Pause goal" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Delete goal" })).toBeNull();
});

it.each([
  {
    capability: undefined,
    actions: [],
    status: "active" as const,
    budget: false,
  },
  {
    capability: {
      observes: true,
      set: false,
      pause: true,
      resume: false,
      tokenBudget: false,
      clear: true,
    },
    status: "active" as const,
    budget: false,
    actions: ["Pause goal", "Delete goal"],
  },
  {
    capability: {
      observes: true,
      set: true,
      pause: false,
      resume: false,
      tokenBudget: false,
      clear: true,
    },
    status: "active" as const,
    budget: false,
    actions: ["Edit goal", "Delete goal"],
  },
  {
    capability: {
      observes: true,
      set: true,
      pause: false,
      resume: false,
      tokenBudget: false,
      clear: false,
    },
    status: "active" as const,
    budget: false,
    actions: ["Edit goal"],
  },
  ...(
    [
      "active",
      "paused",
      "blocked",
      "usageLimited",
      "budgetLimited",
      "complete",
    ] as const
  ).flatMap((status) =>
    [true, false].map((enabled) => ({
      capability: {
        observes: true,
        set: true,
        pause: true,
        resume: enabled,
        tokenBudget: enabled,
        clear: true,
      },
      status,
      budget: enabled,
      actions: [
        "Edit goal",
        "Delete goal",
        ...(status === "active" ? ["Pause goal"] : []),
        ...(enabled &&
        ["paused", "usageLimited", "budgetLimited"].includes(status)
          ? ["Resume goal"]
          : []),
      ],
    })),
  ),
  {
    capability: {
      observes: true,
      set: false,
      pause: false,
      resume: false,
      tokenBudget: true,
      clear: false,
    },
    status: "paused" as const,
    budget: true,
    actions: ["Edit goal budget"],
  },
])(
  "renders only the advertised goal operations: $actions",
  async ({ capability, actions, status, budget }) => {
    render(
      <ChatGoalControl
        state={{
          kind: "goal",
          snapshot: {
            objective: "Ship",
            status,
            tokenBudget: null,
            tokensUsed: 0,
            timeUsedSeconds: 0,
            createdAt: null,
            updatedAt: null,
          },
        }}
        capabilities={capability}
        canMutate
        onMutation={async () => ({ kind: "applied" })}
      />,
    );
    for (const label of [
      "Edit goal",
      "Edit goal budget",
      "Pause goal",
      "Resume goal",
      "Delete goal",
    ]) {
      expect(screen.queryByRole("button", { name: label }) !== null).toBe(
        new Set<string>(actions).has(label),
      );
    }
    const labels: Record<AcpGoalSnapshot["status"], string> = {
      active: "Pursuing goal",
      paused: "Goal paused",
      blocked: "Goal blocked",
      usageLimited: "Goal usage limit reached",
      budgetLimited: "Goal token budget reached",
      complete: "Goal complete",
    };
    expect(screen.getByText(labels[status])).toBeVisible();
    const edit = screen.queryByRole("button", { name: /^Edit goal/ });
    if (edit) {
      fireEvent.click(edit);
      await screen.findByLabelText("Goal objective");
      expect(screen.queryByLabelText("Token budget (optional)") !== null).toBe(
        budget,
      );
    }
  },
);

it("sets a goal from the shared editor without inventing observed goal state", async () => {
  render(
    <ChatGoalControl
      state={{ kind: "noGoal" }}
      capabilities={{
        observes: true,
        set: true,
        pause: false,
        resume: false,
        tokenBudget: false,
        clear: true,
      }}
      canMutate
      editorOpen
      onMutation={async (action, objective) => ({
        kind: "refused",
        message: `${action}: ${objective}`,
      })}
    />,
  );
  fireEvent.change(await screen.findByLabelText("Goal objective"), {
    target: { value: "Ship safely" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save goal" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "set: Ship safely",
  );
  expect(screen.queryByText("Pursuing goal")).toBeNull();
});

it("edits only the budget when the objective is unchanged and rejects invalid budgets", async () => {
  render(
    <ChatGoalControl
      state={{
        kind: "goal",
        snapshot: {
          objective: "Ship safely",
          status: "budgetLimited",
          tokenBudget: 1000,
          tokensUsed: 1000,
          timeUsedSeconds: 10,
          createdAt: null,
          updatedAt: null,
        },
      }}
      capabilities={{
        observes: true,
        set: true,
        pause: true,
        resume: true,
        tokenBudget: true,
        clear: true,
      }}
      canMutate
      editorOpen
      onMutation={async (action, objective, tokenBudget) => ({
        kind: "refused",
        message: `${action}: ${objective ?? "unchanged"}: ${tokenBudget}`,
      })}
    />,
  );
  const input = await screen.findByLabelText("Token budget (optional)");
  for (const value of ["0", "-1", "1.5", "9007199254740992"]) {
    fireEvent.change(input, { target: { value } });
    expect(screen.getByRole("button", { name: "Save goal" })).toBeDisabled();
  }
  fireEvent.change(input, { target: { value: "9007199254740991" } });
  fireEvent.click(screen.getByRole("button", { name: "Save goal" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "tokenBudget: unchanged: 9007199254740991",
  );
  fireEvent.change(await screen.findByLabelText("Goal objective"), {
    target: { value: "New objective" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save goal" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "set: New objective: 9007199254740991",
  );
  fireEvent.click(screen.getByRole("button", { name: "Resume goal" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "resume: unchanged: undefined",
  );
});
