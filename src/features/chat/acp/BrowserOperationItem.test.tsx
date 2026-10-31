import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { browserOperationFixture } from "../../../domain/browserOperation.fixture";
import { BrowserOperationItem } from "./BrowserOperationItem";

afterEach(cleanup);

it("reads a call refused while the user holds the tab as paused, not blocked", () => {
  const { rerender } = render(
    <BrowserOperationItem
      operation={{
        ...browserOperationFixture,
        phase: "denied",
        resultSummary: "Paused: you took over this tab",
      }}
    />,
  );
  expect(
    screen.getByRole("group", {
      name: "Paused: click element on example.com · Paused",
    }),
  ).toBeInTheDocument();
  rerender(
    <BrowserOperationItem
      operation={{
        ...browserOperationFixture,
        phase: "denied",
        resultSummary: "Browser access denied",
      }}
    />,
  );
  expect(
    screen.getByRole("group", { name: /^Blocked: .* · Denied$/ }),
  ).toBeInTheDocument();
});

it("shows an agent browser action with its target and expandable redacted detail", () => {
  render(<BrowserOperationItem operation={browserOperationFixture} />);
  expect(
    screen.getByRole("group", {
      name: "Clicked element on example.com · Completed",
    }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByText("Clicked element on example.com"));
  expect(screen.getByText("Action completed")).toBeInTheDocument();
});

it("shows an unsupported action as a generic failure without its private detail", () => {
  render(
    <BrowserOperationItem
      operation={{
        ...browserOperationFixture,
        operation: "unsupported",
        phase: "failed",
        summary: "Unsupported browser operation",
        resultSummary:
          "This browser operation is not supported by this version of Sage.",
      }}
    />,
  );
  expect(
    screen.getByRole("group", {
      name: "Couldn't perform unsupported browser operation · Failed",
    }),
  ).toBeInTheDocument();
  fireEvent.click(
    screen.getByText("Couldn't perform unsupported browser operation"),
  );
  expect(
    screen.getByText(
      "This browser operation is not supported by this version of Sage.",
    ),
  ).toBeInTheDocument();
});

it("shows a captured click label and tab without exposing a fill value", () => {
  const { rerender } = render(
    <BrowserOperationItem
      operation={{ ...browserOperationFixture, summary: 'Click "Submit"' }}
    />,
  );
  expect(
    screen.getByRole("group", {
      name: 'Clicked "Submit" on example.com · Completed',
    }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByText('Clicked "Submit" on example.com'));
  expect(screen.getByText("Tab browser-7")).toBeInTheDocument();
  rerender(
    <BrowserOperationItem
      operation={{
        ...browserOperationFixture,
        summary: "Fill field private-password",
        resultSummary: "private-password",
      }}
    />,
  );
  expect(
    screen.getByRole("group", {
      name: "Typed into field on example.com · Completed",
    }),
  ).toBeInTheDocument();
  expect(screen.queryByText(/private-password/)).not.toBeInTheDocument();
});

it("shows only the navigation origin, never a sensitive path", () => {
  render(
    <BrowserOperationItem
      operation={{
        ...browserOperationFixture,
        operation: "navigate",
        summary: "Navigate to /reset/short-code",
      }}
    />,
  );
  expect(
    screen.getByRole("group", {
      name: "Navigated to example.com · Completed",
    }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByText("Navigated to example.com"));
  expect(screen.getByText("Origin https://example.com")).toBeInTheDocument();
  expect(screen.queryByText(/short-code/)).not.toBeInTheDocument();
});

it("shows an unapplied fill as failed without showing the value", () => {
  render(
    <BrowserOperationItem
      operation={{
        ...browserOperationFixture,
        phase: "failed",
        summary: "Fill field",
        resultSummary: "Action not applied",
      }}
    />,
  );
  expect(
    screen.getByRole("group", {
      name: "Couldn't type into field on example.com · Failed",
    }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByText("Couldn't type into field on example.com"));
  expect(screen.getByText("Action not applied")).toBeInTheDocument();
});

it("does not attach screenshot bytes to a browser transcript row", () => {
  render(
    <BrowserOperationItem
      operation={{
        ...browserOperationFixture,
        operation: "screenshot",
        summary: "Capture screenshot",
        resultSummary: "jpegBase64=private-image-bytes",
      }}
    />,
  );
  fireEvent.click(screen.getByText("Captured page on example.com"));
  expect(screen.queryByText(/private-image-bytes/)).not.toBeInTheDocument();
  expect(screen.queryByRole("img")).not.toBeInTheDocument();
});

it("marks an unknown outcome as a warning and a denied action as denied", () => {
  const { rerender } = render(
    <BrowserOperationItem
      operation={{ ...browserOperationFixture, phase: "outcomeUnknown" }}
    />,
  );
  expect(
    screen.getByRole("status", { name: "Warning: Outcome unknown" }),
  ).toBeInTheDocument();
  rerender(
    <BrowserOperationItem
      operation={{ ...browserOperationFixture, phase: "denied" }}
    />,
  );
  expect(screen.getByRole("status", { name: "Denied" })).toBeInTheDocument();
});

it.each([
  ["started", "Clicking element on example.com…", "In progress"],
  ["completed", "Clicked element on example.com", "Completed"],
  ["denied", "Blocked: click element on example.com", "Denied"],
  ["failed", "Couldn't click element on example.com", "Failed"],
  [
    "outcomeUnknown",
    "Click element on example.com — result unknown",
    "Outcome unknown",
  ],
] as const)(
  "does not imply success when a browser action is %s",
  (phase, title, status) => {
    render(
      <BrowserOperationItem
        operation={{ ...browserOperationFixture, phase }}
      />,
    );
    expect(
      screen.getByRole("group", { name: `${title} · ${status}` }),
    ).toBeInTheDocument();
  },
);
