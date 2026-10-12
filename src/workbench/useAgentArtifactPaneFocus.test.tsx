import { useRef } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useAgentArtifactPaneFocus } from "./useAgentArtifactPaneFocus";

afterEach(cleanup);

it("focuses resources when an expanded overlay is initially restored", () => {
  render(<ResourceFocusFixture overlay />);
  expect(screen.getByRole("complementary")).toHaveFocus();
});

function ResourceFocusFixture({
  expanded = true,
  overlay,
  onExpandedChange = () => undefined,
  onParentKeyDown = () => undefined,
}: {
  expanded?: boolean;
  overlay: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  onParentKeyDown?: () => void;
}) {
  const paneRef = useRef<HTMLElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const onKeyDown = useAgentArtifactPaneFocus({
    expanded,
    overlay,
    paneRef,
    triggerRef,
    onExpandedChange,
  });
  return (
    <div onKeyDown={onParentKeyDown}>
      <textarea aria-label="Composer" />
      <button ref={triggerRef}>Show files and preview</button>
      {expanded ? (
        <aside ref={paneRef} tabIndex={-1} onKeyDown={onKeyDown}>
          <button>First resource control</button>
          <iframe title="Preview" />
        </aside>
      ) : null}
    </div>
  );
}

it("does not steal composer focus when inline resources become an overlay", () => {
  const view = render(<ResourceFocusFixture overlay={false} />);
  const composer = screen.getByRole("textbox", { name: "Composer" });
  composer.focus();
  view.rerender(<ResourceFocusFixture overlay />);
  expect(composer).toHaveFocus();
});

it("moves pane focus on resize only when a resource control was focused", () => {
  const view = render(<ResourceFocusFixture overlay={false} />);
  screen.getByRole("button", { name: "First resource control" }).focus();
  view.rerender(<ResourceFocusFixture overlay />);
  expect(screen.getByRole("complementary")).toHaveFocus();
});

it("focuses explicitly opened overlays and returns to their trigger on close", () => {
  const view = render(<ResourceFocusFixture expanded={false} overlay />);
  view.rerender(<ResourceFocusFixture expanded overlay />);
  expect(screen.getByRole("complementary")).toHaveFocus();
  view.rerender(<ResourceFocusFixture expanded={false} overlay />);
  expect(
    screen.getByRole("button", { name: "Show files and preview" }),
  ).toHaveFocus();
});

it("keeps forward and reverse Tab inside the overlay including Preview", () => {
  const onExpandedChange = vi.fn();
  render(<ResourceFocusFixture overlay onExpandedChange={onExpandedChange} />);
  const pane = screen.getByRole("complementary");
  const first = screen.getByRole("button", { name: "First resource control" });
  const preview = screen.getByTitle("Preview");
  first.focus();
  fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
  expect(preview).toHaveFocus();
  fireEvent.keyDown(preview, { key: "Tab" });
  expect(first).toHaveFocus();
  pane.focus();
  fireEvent.keyDown(pane, { key: "Tab" });
  expect(first).toHaveFocus();
  expect(onExpandedChange).not.toHaveBeenCalled();
});

it.each([false, true])(
  "only stops Escape propagation for overlay=%s",
  (overlay) => {
    const onParentKeyDown = vi.fn();
    const onExpandedChange = vi.fn();
    render(
      <ResourceFocusFixture
        overlay={overlay}
        onParentKeyDown={onParentKeyDown}
        onExpandedChange={onExpandedChange}
      />,
    );
    fireEvent.keyDown(screen.getByRole("complementary"), { key: "Escape" });
    expect(onExpandedChange).toHaveBeenCalledWith(false);
    expect(onParentKeyDown).toHaveBeenCalledTimes(overlay ? 0 : 1);
  },
);
