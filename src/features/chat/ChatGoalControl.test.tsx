import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
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
  { capability: undefined, actions: [] },
  {
    capability: {
      observes: true,
      set: false,
      pause: true,
      resume: false,
      tokenBudget: false,
      clear: true,
    },
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
    actions: ["Edit goal"],
  },
])(
  "renders only the advertised goal operations: $actions",
  ({ capability, actions }) => {
    render(
      <ChatGoalControl
        state={{
          kind: "goal",
          snapshot: {
            objective: "Ship",
            status: "active",
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
    for (const label of ["Edit goal", "Pause goal", "Delete goal"]) {
      expect(screen.queryByRole("button", { name: label }) !== null).toBe(
        new Set<string>(actions).has(label),
      );
    }
    expect(screen.queryByRole("button", { name: /resume|budget/i })).toBeNull();
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
