import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { BrowserNewTab } from "./BrowserNewTab";

afterEach(cleanup);

it("offers five Sage tools with real shortcuts and no invented history", () => {
  const onTool = vi.fn();
  render(
    <BrowserNewTab
      suggestions={[]}
      onNavigate={vi.fn()}
      onNewTab={vi.fn()}
      onTool={onTool}
      chatAvailable
      sideChatAvailable={false}
      terminalAvailable
    />,
  );
  for (const [name, shortcut, tool] of [
    ["Changes", "Control+Shift+G Meta+Shift+G", "changes"],
    ["Terminal", "Control+` Meta+`", "terminal"],
    ["Files", "Control+P Meta+P", "files"],
  ]) {
    const button = screen.getByRole("button", { name });
    expect(button).toHaveAttribute("aria-keyshortcuts", shortcut);
    fireEvent.click(button);
    expect(onTool).toHaveBeenLastCalledWith(tool);
  }
  expect(screen.getByRole("button", { name: "Side chat" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "New page" })).toBeEnabled();
  expect(screen.queryByText("Suggested")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Terminal options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Open terminal dock" }));
  expect(onTool).toHaveBeenLastCalledWith("terminal");
});

it("navigates supplied history and opens a real new page", () => {
  const onNavigate = vi.fn();
  const onNewTab = vi.fn();
  render(
    <BrowserNewTab
      suggestions={[{ url: "https://example.com/", title: "Example Domain" }]}
      onNavigate={onNavigate}
      onNewTab={onNewTab}
      onTool={vi.fn()}
      chatAvailable
      sideChatAvailable
      terminalAvailable={false}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Example Domain" }));
  expect(onNavigate).toHaveBeenCalledWith("https://example.com/");
  fireEvent.click(screen.getByRole("button", { name: "New page" }));
  expect(onNewTab).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "Terminal" })).toBeDisabled();
});

it("disables every Chat launcher when no Agent Chat owns the New tab", () => {
  render(
    <BrowserNewTab
      suggestions={[]}
      onNavigate={vi.fn()}
      onNewTab={vi.fn()}
      onTool={vi.fn()}
      chatAvailable={false}
      sideChatAvailable
      terminalAvailable
    />,
  );
  for (const name of ["Changes", "Terminal", "Files", "Side chat"]) {
    const button = screen.getByRole("button", { name });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", "Open a Chat in Agent mode first");
  }
  expect(screen.getByRole("button", { name: "New page" })).toBeEnabled();
});
