import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
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
