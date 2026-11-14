import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { ResourceStateNotice } from "./ResourceStateNotice";

afterEach(() => {
  cleanup();
});

describe("ResourceStateNotice", () => {
  it("renders idle as a quiet status notice without a loader or alert icon", () => {
    render(
      <ResourceStateNotice
        state="idle"
        title="Preview is idle"
        detail="Preview shows loopback development servers started from this chat."
      />,
    );

    const notice = screen.getByRole("status");
    expect(notice).toHaveClass("resource-state--idle");
    expect(notice).toHaveAttribute("aria-atomic", "true");
    expect(notice.querySelector("svg")).toBeNull();
    expect(notice.querySelector(".resource-state__icon")).toBeNull();
    expect(screen.getByText("Preview is idle")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Preview shows loopback development servers started from this chat.",
      ),
    ).toBeInTheDocument();
  });

  it("preserves loading status semantics with its loader", () => {
    render(<ResourceStateNotice state="loading" title="Loading diff…" />);

    const notice = screen.getByRole("status");
    expect(notice).toHaveClass("resource-state--loading");
    expect(notice.querySelector("svg")).not.toBeNull();
    expect(notice.querySelector(".resource-state__icon")).not.toBeNull();
    expect(screen.getByText("Loading diff…")).toBeInTheDocument();
  });

  it("preserves error alert semantics with its alert icon and detail", () => {
    render(
      <ResourceStateNotice
        state="error"
        title="Could not load this diff"
        detail="The comparison is temporarily unavailable."
      />,
    );

    const notice = screen.getByRole("alert");
    expect(notice).toHaveClass("resource-state--error");
    expect(notice.querySelector("svg")).not.toBeNull();
    expect(screen.getByText("Could not load this diff")).toBeInTheDocument();
    expect(
      screen.getByText("The comparison is temporarily unavailable."),
    ).toBeInTheDocument();
  });
});
