import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { createRef, useRef } from "react";
import { afterEach, expect, it, vi } from "vitest";

import { useAgentArtifactPaneFocus } from "../../workbench/useAgentArtifactPaneFocus";
import { BrowserNavigationControls } from "./BrowserNavigationControls";

vi.mock("../../ports/nativeInvoke", () => ({ isTauriRuntime: () => false }));

afterEach(cleanup);

function Pane({
  onExpandedChange,
  onMenuOpenChange,
}: {
  onExpandedChange: (expanded: boolean) => void;
  onMenuOpenChange?: (open: boolean) => void;
}) {
  const paneRef = useRef<HTMLElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const onKeyDown = useAgentArtifactPaneFocus({
    expanded: true,
    overlay: false,
    paneRef,
    triggerRef,
    onExpandedChange,
  });
  return (
    <section ref={paneRef} onKeyDown={onKeyDown}>
      <div className="browser-surface__toolbar">
        <BrowserNavigationControls
          navigation={null}
          page={null}
          address=""
          addressField={createRef()}
          onAddressChange={vi.fn()}
          onNavigate={vi.fn()}
          onControl={vi.fn()}
          onError={vi.fn()}
          onFind={vi.fn()}
          onPrint={vi.fn()}
          onScreenshot={vi.fn()}
          onZoom={vi.fn()}
          onZoomReset={vi.fn()}
          deviceOpen={false}
          onDevice={vi.fn()}
          onDownloads={vi.fn()}
          onHistory={vi.fn()}
          onSettings={vi.fn()}
          onCopy={vi.fn()}
          onExternal={vi.fn()}
          clearDataControl={null}
          onMenuOpenChange={onMenuOpenChange}
        />
      </div>
      <p>Page area</p>
    </section>
  );
}

it("closes only the … menu on Escape and keeps the right pane open (AP-A4)", async () => {
  const onExpandedChange = vi.fn();
  render(<Pane onExpandedChange={onExpandedChange} />);
  const trigger = screen.getByRole("button", { name: "More options" });
  fireEvent.click(trigger);
  const first = screen.getByRole("menuitem", { name: "Downloads" });
  await waitFor(() => expect(first).toHaveFocus());

  fireEvent.keyDown(first, { key: "Escape" });

  expect(screen.queryByRole("menu")).toBeNull();
  expect(trigger).toHaveFocus();
  expect(onExpandedChange).not.toHaveBeenCalled();

  fireEvent.keyDown(trigger, { key: "Escape" });
  expect(onExpandedChange).toHaveBeenCalledWith(false);
});

it("reports the menu as open so the page can hide, and closes it on an outside pointer", () => {
  const onMenuOpenChange = vi.fn();
  render(
    <Pane onExpandedChange={vi.fn()} onMenuOpenChange={onMenuOpenChange} />,
  );
  expect(onMenuOpenChange).toHaveBeenLastCalledWith(false);
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  expect(onMenuOpenChange).toHaveBeenLastCalledWith(true);

  fireEvent.pointerDown(screen.getByRole("menuitem", { name: "History" }));
  expect(screen.getByRole("menu")).toBeInTheDocument();

  fireEvent.pointerDown(screen.getByText("Page area"));
  expect(screen.queryByRole("menu")).toBeNull();
  expect(onMenuOpenChange).toHaveBeenLastCalledWith(false);
});
