import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { page } from "./acpProjection.fixture";
import { TurnFailure } from "./TurnFailure";

afterEach(cleanup);

describe("TurnFailure", () => {
  it("shows the failure message, remediation and existing retry action", () => {
    const onRetry = vi.fn();
    render(
      <TurnFailure
        turn={
          page("failed", {
            kind: "authRequired",
            message: "Authentication required",
            remediation: "Sign in to continue",
          }).turns[0]
        }
        onRetry={onRetry}
      />,
    );
    const failure = screen.getByRole("group", { name: "Turn failed" });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(failure).not.toHaveAttribute("aria-label");
    expect(within(failure).getByText("Authentication required")).toBeVisible();
    expect(within(failure).getByText("Sign in to continue")).toBeVisible();
    fireEvent.click(within(failure).getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("omits absent details and retry without a callback", () => {
    const { container } = render(
      <TurnFailure turn={page("failed").turns[0]} />,
    );
    expect(
      screen.getByRole("group", { name: "Turn failed" }),
    ).toHaveTextContent("Turn failed");
    expect(container.querySelector("p")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("shows no failure box for a completed turn", () => {
    render(<TurnFailure turn={page("completed").turns[0]} onRetry={vi.fn()} />);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("group", { name: "Turn failed" })).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
