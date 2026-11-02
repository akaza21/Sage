import { useRef, useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useAgentArtifactPaneFocus } from "./useAgentArtifactPaneFocus";
import { BrowserNavigationControls } from "../features/browser/BrowserNavigationControls";

afterEach(cleanup);

function BrowserResourceFixture({
  overlay,
  onExpandedChange,
}: {
  overlay: boolean;
  onExpandedChange: (expanded: boolean) => void;
}) {
  const paneRef = useRef<HTMLElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const addressField = useRef<HTMLInputElement>(null);
  const [expanded, setExpanded] = useState(true);
  const onKeyDown = useAgentArtifactPaneFocus({
    expanded,
    overlay,
    paneRef,
    triggerRef,
    onExpandedChange: (next) => {
      setExpanded(next);
      onExpandedChange(next);
    },
  });
  const noop = () => undefined;
  return expanded ? (
    <aside ref={paneRef} tabIndex={-1} onKeyDown={onKeyDown}>
      <BrowserNavigationControls
        navigation={null}
        page={null}
        address=""
        addressField={addressField}
        onAddressChange={noop}
        onNavigate={noop}
        onControl={noop}
        onError={noop}
        onFind={noop}
        onPrint={noop}
        onScreenshot={noop}
        onZoom={noop}
        onZoomReset={noop}
        onDownloads={noop}
        onHistory={noop}
        onSettings={noop}
        onCopy={noop}
        onExternal={noop}
        clearDataControl={null}
      />
    </aside>
  ) : null;
}

it.each([false, true])(
  "closes the real browser More menu before the resource pane (overlay=%s)",
  (overlay) => {
    const onExpandedChange = vi.fn();
    render(
      <BrowserResourceFixture
        overlay={overlay}
        onExpandedChange={onExpandedChange}
      />,
    );
    const trigger = screen.getByRole("button", { name: "More options" });
    fireEvent.click(trigger);
    fireEvent.keyDown(screen.getByRole("menu", { name: "Browser menu" }), {
      key: "Escape",
    });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
    expect(onExpandedChange).not.toHaveBeenCalled();
    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(onExpandedChange).toHaveBeenCalledOnce();
    expect(onExpandedChange).toHaveBeenCalledWith(false);
  },
);

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
          <button
            onKeyDown={(event) => {
              if (event.key === "Escape") event.preventDefault();
            }}
          >
            Nested layer
          </button>
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

it("restores an automatically collapsed pane when inline space returns", () => {
  function ResizablePane({ overlay }: { overlay: boolean }) {
    const [expanded, setExpanded] = useState(true);
    return (
      <ResourceFocusFixture
        expanded={expanded}
        overlay={overlay}
        onExpandedChange={setExpanded}
      />
    );
  }
  const view = render(<ResizablePane overlay={false} />);
  const composer = screen.getByRole("textbox", { name: "Composer" });
  composer.focus();
  view.rerender(<ResizablePane overlay />);
  expect(screen.queryByRole("complementary")).toBeNull();
  view.rerender(<ResizablePane overlay={false} />);
  expect(screen.getByRole("complementary")).toBeVisible();
  expect(composer).toHaveFocus();
});

it.each([false, true])(
  "Escape in the browser address stays in the address control (overlay=%s)",
  (overlay) => {
    render(
      <BrowserResourceFixture overlay={overlay} onExpandedChange={() => {}} />,
    );
    const address = screen.getByRole("textbox", { name: "Address or search" });
    address.focus();
    fireEvent.change(address, { target: { value: "draft" } });
    fireEvent.keyDown(address, { key: "Escape" });
    expect(screen.getByRole("complementary")).toBeVisible();
    expect(address).not.toHaveFocus();
  },
);

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
  "consumes Escape after closing the pane for overlay=%s",
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
    expect(onParentKeyDown).not.toHaveBeenCalled();
  },
);

it.each([false, true])(
  "leaves the pane open when an inner layer consumes Escape (overlay=%s)",
  (overlay) => {
    const onExpandedChange = vi.fn();
    render(
      <ResourceFocusFixture
        overlay={overlay}
        onExpandedChange={onExpandedChange}
      />,
    );
    fireEvent.keyDown(screen.getByRole("button", { name: "Nested layer" }), {
      key: "Escape",
    });
    expect(onExpandedChange).not.toHaveBeenCalled();
  },
);
