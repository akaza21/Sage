import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createRef, useState } from "react";
import { AgentArtifactPaneControls } from "./AgentArtifactPaneControls";

afterEach(cleanup);
it("expands the actual resource pane, restores it, and opens a real browser tab", () => {
  const paneRef = createRef<HTMLElement>();
  const onNewTab = vi.fn();
  const onCollapse = vi.fn();
  function Pane() {
    const [maximized, setMaximized] = useState(false);
    return (
      <aside
        ref={paneRef}
        className={maximized ? "agent-artifact-pane--maximized" : ""}
      >
        <AgentArtifactPaneControls
          maximized={maximized}
          onMaximizedChange={setMaximized}
          disabled={false}
          onNewTab={onNewTab}
          onCollapse={onCollapse}
        />
      </aside>
    );
  }
  render(<Pane />);
  fireEvent.click(screen.getByRole("button", { name: "Expand resource pane" }));
  expect(paneRef.current).toHaveClass("agent-artifact-pane--maximized");
  fireEvent.click(
    screen.getByRole("button", { name: "Restore resource pane" }),
  );
  expect(paneRef.current).not.toHaveClass("agent-artifact-pane--maximized");
  const newTab = screen.getByRole("button", { name: "New browser tab" });
  expect(newTab).toHaveClass("agent-artifact-pane__new-tab");
  fireEvent.click(newTab);
  expect(onNewTab).toHaveBeenCalledOnce();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Collapse files, changes, and preview",
    }),
  );
  expect(onCollapse).toHaveBeenCalledOnce();
});

it("makes the covered Chat and rail inert only while the pane is expanded", () => {
  function Body() {
    const [maximized, setMaximized] = useState(false);
    return (
      <div>
        <nav data-testid="rail" />
        <main data-testid="chat" />
        <div className="agent-artifact-surface">
          <aside className="agent-artifact-pane">
            <AgentArtifactPaneControls
              maximized={maximized}
              onMaximizedChange={setMaximized}
              disabled={false}
              onNewTab={vi.fn()}
              onCollapse={vi.fn()}
            />
          </aside>
        </div>
      </div>
    );
  }
  render(<Body />);
  const rail = screen.getByTestId("rail");
  const chat = screen.getByTestId("chat");
  expect(chat).not.toHaveAttribute("inert");
  fireEvent.click(screen.getByRole("button", { name: "Expand resource pane" }));
  expect(rail).toHaveAttribute("inert");
  expect(chat).toHaveAttribute("inert");
  fireEvent.click(
    screen.getByRole("button", { name: "Restore resource pane" }),
  );
  expect(rail).not.toHaveAttribute("inert");
  expect(chat).not.toHaveAttribute("inert");
});
