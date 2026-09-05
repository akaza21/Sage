import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { HarnessBrowserSurface } from "./HarnessBrowserSurface";
import { applyBrowseIntent, clearBrowseSessions } from "./harnessBrowse";
import type { AgentArtifactOwner } from "../../domain/agentArtifact";

const ownerA = {
  projectId: "project-1",
  chatId: "chat:a",
  sessionId: "session-1",
} as AgentArtifactOwner;
const ownerB = {
  projectId: "project-1",
  chatId: "chat:b",
  sessionId: "session-1",
} as AgentArtifactOwner;

afterEach(() => {
  cleanup();
  clearBrowseSessions();
});

describe("HarnessBrowserSurface", () => {
  it("shows the admitted loopback page for that chat and keeps per-Chat idle isolation", () => {
    applyBrowseIntent({
      owner: ownerA,
      url: "http://127.0.0.1:5173/app",
    });
    const { rerender } = render(<HarnessBrowserSurface owner={ownerA} />);
    const frame = screen.getByTitle("Sage browser");
    expect(frame).toHaveAttribute("src", "http://127.0.0.1:5173/app");
    expect(screen.getByLabelText("Preview address")).toHaveValue(
      "http://127.0.0.1:5173/app",
    );

    rerender(<HarnessBrowserSurface owner={ownerB} />);
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(screen.getByText("Preview is idle")).toBeInTheDocument();
    expect(
      screen.getByText(
        /Preview shows loopback development servers started from this Chat/,
      ),
    ).toBeInTheDocument();
  });

  it("renders idle as a quiet status notice without a loader or alert icon", () => {
    render(<HarnessBrowserSurface owner={ownerA} />);

    const notice = screen.getByRole("status");
    expect(notice).toHaveClass("resource-state--idle");
    expect(notice.querySelector("svg")).toBeNull();
    expect(screen.getByText("Preview is idle")).toBeInTheDocument();
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Reload preview" }),
    ).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Close browser" })).toBeNull();
  });

  it("renders unavailable when no Chat owns Preview", () => {
    render(<HarnessBrowserSurface owner={null} />);

    expect(screen.queryByTitle("Sage browser")).toBeNull();
    const notice = screen.getByRole("status");
    expect(notice).toHaveClass("resource-state--idle");
    expect(notice.querySelector("svg")).toBeNull();
    expect(screen.getByText("Preview unavailable")).toBeInTheDocument();
    expect(screen.getByLabelText("Preview address")).toBeDisabled();
  });

  it("opens an admitted address and refuses public HTTPS", () => {
    render(<HarnessBrowserSurface owner={ownerA} />);
    const address = screen.getByLabelText("Preview address");
    fireEvent.change(address, {
      target: { value: "https://example.com/docs" },
    });
    fireEvent.submit(address.closest("form")!);
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Only loopback HTTP(S) with a port",
    );

    fireEvent.change(address, {
      target: { value: "http://127.0.0.1:4173/preview" },
    });
    fireEvent.submit(address.closest("form")!);
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:4173/preview",
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("reloads the admitted page without leaving Sage", () => {
    applyBrowseIntent({
      owner: ownerA,
      url: "http://127.0.0.1:5173/app",
    });
    render(<HarnessBrowserSurface owner={ownerA} />);
    const first = screen.getByTitle("Sage browser");
    fireEvent.click(screen.getByRole("button", { name: "Reload preview" }));
    const reloaded = screen.getByTitle("Sage browser");
    expect(reloaded).toHaveAttribute("src", "http://127.0.0.1:5173/app");
    expect(reloaded).not.toBe(first);
  });

  it("keeps the browser chrome controls named in DOM order", () => {
    applyBrowseIntent({
      owner: ownerA,
      url: "http://127.0.0.1:5173/app",
    });
    render(<HarnessBrowserSurface owner={ownerA} />);

    const chrome = screen.getByRole("region", { name: "Sage browser" });
    expect(
      Array.from(chrome.querySelectorAll("button, input")).map((element) =>
        element.getAttribute("aria-label"),
      ),
    ).toEqual([
      "Back",
      "Forward",
      "Preview address",
      "Reload preview",
      "Close browser",
    ]);
  });

  it("keeps idle chrome order without a Close control", () => {
    render(<HarnessBrowserSurface owner={ownerA} />);

    const chrome = screen.getByRole("region", { name: "Sage browser" });
    expect(
      Array.from(chrome.querySelectorAll("button, input")).map((element) =>
        element.getAttribute("aria-label"),
      ),
    ).toEqual(["Back", "Forward", "Preview address", "Reload preview"]);
  });

  it("walks admitted back and forward without leaving Sage", () => {
    applyBrowseIntent({
      owner: ownerA,
      url: "http://127.0.0.1:5173/one",
    });
    applyBrowseIntent({
      owner: ownerA,
      url: "http://127.0.0.1:5173/two",
    });
    render(<HarnessBrowserSurface owner={ownerA} />);
    const back = screen.getByRole("button", { name: "Back" });
    const forward = screen.getByRole("button", { name: "Forward" });
    expect(back).toBeEnabled();
    expect(forward).toBeDisabled();
    fireEvent.click(back);
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/one",
    );
    expect(screen.getByLabelText("Preview address")).toHaveValue(
      "http://127.0.0.1:5173/one",
    );
    expect(forward).toBeEnabled();
    fireEvent.click(forward);
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/two",
    );
  });

  it("clears the chat session without opening an external window", () => {
    applyBrowseIntent({
      owner: ownerA,
      url: "http://127.0.0.1:4173/",
    });
    render(<HarnessBrowserSurface owner={ownerA} />);
    fireEvent.click(screen.getByRole("button", { name: "Close browser" }));
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(screen.getByText("Preview is idle")).toBeInTheDocument();
  });

  it("returns focus to the Preview address after closing", () => {
    applyBrowseIntent({
      owner: ownerA,
      url: "http://127.0.0.1:4173/",
    });
    render(<HarnessBrowserSurface owner={ownerA} />);
    fireEvent.click(screen.getByRole("button", { name: "Close browser" }));
    expect(screen.getByLabelText("Preview address")).toHaveFocus();
    expect(screen.getByText("Preview is idle")).toBeInTheDocument();
  });
});
