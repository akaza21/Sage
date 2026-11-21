import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { Notice } from "./Notice";
import { StrictMode } from "react";

afterEach(cleanup);

it("announces a blocking problem once across copy updates and announces a new identity", () => {
  const { rerender } = render(
    <Notice
      identity="read:file-a"
      severity="blocking"
      message="File could not be read."
    />,
  );
  expect(screen.getByRole("alert")).not.toHaveAttribute("aria-live", "off");
  rerender(
    <Notice
      identity="read:file-a"
      severity="blocking"
      message="File is unavailable."
    />,
  );
  expect(screen.getByRole("alert")).toHaveAttribute("aria-live", "off");
  expect(screen.getByText("File is unavailable.")).toBeVisible();
  rerender(
    <Notice
      identity="read:file-a"
      severity="blocking"
      message="File could not be read."
    />,
  );
  expect(screen.getByRole("alert")).toHaveAttribute("aria-live", "off");
  expect(screen.getByText("File could not be read.")).toBeVisible();
  rerender(
    <Notice
      identity="read:file-b"
      severity="blocking"
      message="File is unavailable."
    />,
  );
  expect(screen.getByRole("alert")).not.toHaveAttribute("aria-live", "off");
});

it("announces under StrictMode and restores polite progress after a silenced failure", () => {
  const { rerender } = render(
    <StrictMode>
      <Notice
        identity="history:chat-a"
        severity="blocking"
        message="History unavailable."
      />
    </StrictMode>,
  );
  expect(screen.getByRole("alert")).toHaveAttribute("aria-live", "assertive");
  rerender(
    <StrictMode>
      <Notice
        identity="history:chat-a"
        severity="blocking"
        message="History read failed."
      />
    </StrictMode>,
  );
  expect(screen.getByRole("alert")).toHaveAttribute("aria-live", "off");
  rerender(
    <StrictMode>
      <Notice
        identity="history:chat-a"
        severity="info"
        progress
        message="Loading history…"
      />
    </StrictMode>,
  );
  expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
});

it("mounts diagnostics only while disclosed and resets disclosure for a new problem", () => {
  const { rerender } = render(
    <Notice
      identity="read:file-a"
      severity="warning"
      message="File changed."
      details="Version 4 conflicts with version 5."
    />,
  );
  expect(screen.queryByText("Version 4 conflicts with version 5.")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Details" }));
  expect(screen.getByText("Version 4 conflicts with version 5.")).toBeVisible();
  rerender(
    <Notice
      identity="read:file-b"
      severity="warning"
      message="File changed."
      details="A different conflict."
    />,
  );
  expect(screen.getByRole("button", { name: "Details" })).toHaveAttribute(
    "aria-expanded",
    "false",
  );
  expect(screen.queryByText("A different conflict.")).toBeNull();
});
