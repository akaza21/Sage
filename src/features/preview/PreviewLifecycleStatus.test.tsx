import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { PreviewLifecycleStatus } from "./PreviewLifecycleStatus";

afterEach(cleanup);

it("explains a Preview failure once with its one restart", () => {
  const restart = vi.fn();
  render(
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
