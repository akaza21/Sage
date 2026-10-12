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
import { TurnHeader } from "./TurnHeader";

afterEach(cleanup);

describe("TurnFailure", () => {
  it.each([
    ["authRequired", "Not signed in"],
    ["harnessExited", "Harness exited"],
    ["protocolError", "Protocol error"],
    ["refusedVersion", "Version refused"],
    ["timeout", "Timed out"],
    ["other", "Reason unavailable"],
  ] as const)(
    "states a message-free %s failure cause only once in the turn",
    (kind, label) => {
      const turn = page("failed", { kind }).turns[0];
      render(
        <>
          <TurnHeader turn={turn} />
          <TurnFailure turn={turn} />
        </>,
      );
      expect(screen.getAllByText(label)).toHaveLength(1);
      expect(
        within(screen.getByRole("group", { name: "Turn failed" })).getByText(
          label,
        ),
      ).toBeVisible();
    },
  );

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

  it("states that the reason is unavailable when no cause was supplied", () => {
    render(<TurnFailure turn={page("failed").turns[0]} />);
    expect(
      screen.getByRole("group", { name: "Turn failed" }),
    ).toHaveTextContent("Turn failed");
    expect(screen.getByText("Reason unavailable")).toBeVisible();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it.each([
    ["authRequired", "Not signed in"],
    ["harnessExited", "Harness exited"],
    ["protocolError", "Protocol error"],
    ["refusedVersion", "Version refused"],
    ["timeout", "Timed out"],
    ["other", "Reason unavailable"],
  ] as const)(
    "uses the %s reason as the cause when no text was supplied",
    (kind, label) => {
      render(<TurnFailure turn={page("failed", { kind }).turns[0]} />);
      expect(
        within(screen.getByRole("group", { name: "Turn failed" })).getByText(
          label,
        ),
      ).toBeVisible();
    },
  );

  it.each([
    ["terminal message", "Terminal cause", true],
    ["latest error", "Latest error", false],
  ] as const)(
    "uses the %s before historical error items",
    (_source, expected, hasMessage) => {
      const turn = page("failed", {
        kind: "timeout",
        ...(hasMessage ? { message: "Terminal cause" } : {}),
      }).turns[0];
      turn.items = [
        ...turn.items,
        {
          id: "old-error",
          kind: "error",
          payload: {
            code: "old",
            message: "Earlier error",
            recoverable: false,
          },
        } as never,
        {
          id: "last-error",
          kind: "error",
          lifecycle: "failed",
          payload: {
            code: "latest",
            message: "Latest error",
            recoverable: false,
          },
        } as never,
      ];
      render(<TurnFailure turn={turn} />);
      const failure = screen.getByRole("group", { name: "Turn failed" });
      expect(within(failure).getByText(expected)).toBeVisible();
      expect(failure).not.toHaveTextContent("Earlier error");
      expect(failure).not.toHaveTextContent("Timed out");
      if (hasMessage) expect(failure).not.toHaveTextContent("Latest error");
    },
  );

  it.each([
    [false, "failed"],
    [true, "failed"],
    [false, "completed"],
    [true, "completed"],
  ] as const)(
    "skips an attempt kind with teardown=%s and lifecycle=%s",
    (hasTeardownError, lifecycle) => {
      const turn = page("failed", { kind: "authRequired" }).turns[0];
      turn.items = [
        ...(hasTeardownError
          ? [
              {
                id: "teardown-error",
                kind: "error",
                lifecycle: "failed",
                payload: {
                  code: "auth",
                  message: "Harness authentication failed",
                  recoverable: false,
                },
              } as never,
            ]
          : []),
        {
          id: "retry-error",
          kind: "error",
          lifecycle,
          payload: {
            kind: "attempt",
            code: "Renamed attempt label",
            message: "Earlier retry attempt failed",
            recoverable: false,
          },
        } as never,
      ];
      render(<TurnFailure turn={turn} />);
      const failure = screen.getByRole("group", { name: "Turn failed" });
      expect(
        within(failure).getByText(
          hasTeardownError ? "Harness authentication failed" : "Not signed in",
        ),
      ).toBeVisible();
      expect(failure).not.toHaveTextContent("Earlier retry attempt failed");
    },
  );

  it("does not exclude a display label without an attempt kind", () => {
    const turn = page("failed", { kind: "other" }).turns[0];
    turn.items = [
      {
        kind: "error",
        lifecycle: "failed",
        payload: {
          code: "Attempt failed",
          message: "Harness teardown failed",
          recoverable: false,
        },
      } as never,
    ];
    render(<TurnFailure turn={turn} />);
    expect(screen.getByText("Harness teardown failed")).toBeVisible();
  });

  it("shows no failure box for a completed turn", () => {
    render(<TurnFailure turn={page("completed").turns[0]} onRetry={vi.fn()} />);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("group", { name: "Turn failed" })).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("uses the latest error item for a legacy turn without a terminal reason", () => {
    const turn = page("failed").turns[0];
    turn.items = [
      { kind: "error", payload: { message: "Earlier error" } } as never,
      { kind: "error", payload: { message: "Latest error" } } as never,
    ];
    render(<TurnFailure turn={turn} />);
    const failure = screen.getByRole("group", { name: "Turn failed" });
    expect(within(failure).getByText("Latest error")).toBeVisible();
    expect(failure).not.toHaveTextContent("Earlier error");
  });

  it("treats whitespace-only terminal and error messages as an unavailable cause", () => {
    const turn = page("failed", { kind: "other", message: " \n\t " }).turns[0];
    turn.items = [{ kind: "error", payload: { message: "  " } } as never];
    render(<TurnFailure turn={turn} />);
    expect(screen.getByText("Reason unavailable")).toBeVisible();
  });
});
