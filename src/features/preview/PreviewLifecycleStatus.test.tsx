import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { PreviewLifecycleStatus } from "./PreviewLifecycleStatus";

afterEach(cleanup);

it("reads an exit while watching in the same polite status, without an alert", () => {
  const scope = "project:chat:root:dev";
  const { rerender } = render(
    <PreviewLifecycleStatus
      failure={null}
      phase="healthChecking"
      scope={scope}
    />,
  );
  const status = screen.getByRole("status");
  rerender(
    <PreviewLifecycleStatus
      failure="exitedEarly"
      phase="exited"
      scope={scope}
    />,
  );
  expect(screen.getByRole("status")).toBe(status);
  expect(status).toHaveAttribute("aria-live", "polite");
  expect(status).toHaveClass("notice--warning");
  expect(screen.queryByRole("alert")).toBeNull();
});

it("keeps an exited Preview a quiet warning when it remounts", () => {
  const props = {
    failure: "exitedEarly" as const,
    phase: "exited" as const,
    scope: "project:chat:root:dev",
  };
  const { unmount } = render(<PreviewLifecycleStatus {...props} />);
  unmount();
  render(<PreviewLifecycleStatus {...props} />);
  expect(document.querySelector(".notice")).toHaveClass("notice--warning");
  expect(screen.queryByRole("alert")).toBeNull();
});

it("explains a Preview failure once with its one restart", () => {
  const restart = vi.fn();
  const { rerender } = render(
    <PreviewLifecycleStatus
      failure="healthTimeout"
      phase="healthFailed"
      scope="project:chat:root:dev"
      onRestart={restart}
    />,
  );
  const notices = document.querySelectorAll(".notice");
  expect(notices).toHaveLength(1);
  expect(notices[0]).toHaveClass("notice--warning");
  expect(notices[0]).toHaveAttribute("aria-live", "polite");
  // An inserted alert is what screen readers read; it stays polite.
  expect(screen.getByRole("alert")).toBe(notices[0]);
  rerender(
    <PreviewLifecycleStatus
      failure="healthTimeout"
      phase="healthFailed"
      scope="project:chat:root:dev"
      onRestart={restart}
    />,
  );
  expect(document.querySelector(".notice")).toBe(notices[0]);
  expect(
    screen.getAllByText("Preview did not become healthy before its deadline."),
  ).toHaveLength(1);
  const actions = screen.getAllByRole("button");
  expect(actions).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Start again" }));
  expect(restart).toHaveBeenCalledOnce();
});

it("offers no restart for a failure a fresh start cannot clear", () => {
  render(
    <PreviewLifecycleStatus
      failure="conflict"
      phase="stopped"
      scope="project:chat:root:dev"
      onRestart={vi.fn()}
    />,
  );
  expect(document.querySelector(".notice")).toHaveClass("notice--warning");
  expect(screen.queryByRole("button")).toBeNull();
});

it("keeps ordinary lifecycle progress quiet", () => {
  render(
    <PreviewLifecycleStatus
      compact
      failure={null}
      phase="starting"
      scope="project:chat:root:dev"
      onRestart={vi.fn()}
    />,
  );
  const status = screen.getByRole("status");
  expect(status).toHaveClass("notice--info");
  expect(status).toHaveTextContent("Starting process");
  expect(screen.queryByRole("button")).toBeNull();
});
