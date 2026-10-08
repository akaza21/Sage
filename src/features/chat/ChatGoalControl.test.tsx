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

it("shows an active native Goal above the composer and exposes details", () => {
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
  expect(screen.getByText("42 / 1,000 tokens")).not.toBeVisible();
  screen.getByText("Pursuing goal").click();
  expect(screen.getByText("42 / 1,000 tokens")).toBeVisible();
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
