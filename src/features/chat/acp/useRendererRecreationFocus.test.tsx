import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useRef } from "react";

import type { AcpChatPage } from "../../../domain/acp";
import { useRendererRecreationFocus } from "./useRendererRecreationFocus";

function page(revision: string, chatId = "chat-a"): AcpChatPage {
  return {
    projectId: "project-a",
    chatId,
    projectionRevision: revision,
    turns: [],
    sessions: [],
  } as unknown as AcpChatPage;
}

function FocusHarness({
  value,
  open = true,
  target = true,
}: {
  value: AcpChatPage | null;
  open?: boolean;
  target?: boolean;
}) {
  const surfaceRef = useRef<HTMLElement>(null);
  useRendererRecreationFocus(surfaceRef, value);
  return (
    <section ref={surfaceRef}>
      <details data-sage-activity-id="activity-a" open={open}>
        <summary>Activity summary</summary>
        {target ? (
          <div data-sage-id="item-a" role="treeitem" tabIndex={0}>
            Item
          </div>
        ) : null}
      </details>
    </section>
  );
}

function DualFocusHarness({
  focusedPage,
}: {
  focusedPage: AcpChatPage | null;
}) {
  const focusedRef = useRef<HTMLElement>(null);
  const unfocusedRef = useRef<HTMLElement>(null);
  useRendererRecreationFocus(focusedRef, focusedPage);
  useRendererRecreationFocus(unfocusedRef, null);
  return (
    <>
      <section ref={focusedRef} data-testid="focused-surface">
        <details data-sage-activity-id="activity-a" open>
          <summary>Focused summary</summary>
          <div data-sage-id="item-a" role="treeitem" tabIndex={0}>
            Focused item
          </div>
        </details>
      </section>
      <section ref={unfocusedRef} data-testid="unfocused-surface">
        <details data-sage-activity-id="activity-b" open>
          <summary>Unfocused summary</summary>
          <div data-sage-id="item-b" role="treeitem" tabIndex={0}>
            Unfocused item
          </div>
        </details>
      </section>
    </>
  );
}

afterEach(() => {
  cleanup();
  sessionStorage.clear();
  vi.restoreAllMocks();
});

function saveFocusedTarget(unmount: () => void) {
  const target = document.querySelector<HTMLElement>(
    '[data-sage-id="item-a"]',
  )!;
  target.focus();
  window.dispatchEvent(new Event("pagehide"));
  unmount();
}

describe("useRendererRecreationFocus", () => {
  it("restores the exact Sage ID once after renderer recreation", () => {
    const first = render(<FocusHarness value={page("2")} />);
    saveFocusedTarget(first.unmount);

    const recreated = render(<FocusHarness value={page("2")} />);
    expect(recreated.getByRole("treeitem")).toHaveFocus();
    recreated.getByText("Activity summary").focus();
    recreated.unmount();

    const later = render(<FocusHarness value={page("2")} />);
    expect(later.getByRole("treeitem")).not.toHaveFocus();
  });

  it("falls back to the activity summary when the target is absent or hidden", () => {
    const first = render(<FocusHarness value={page("2")} />);
    saveFocusedTarget(first.unmount);

    const recreated = render(
      <FocusHarness value={page("2")} open={false} target={false} />,
    );
    expect(recreated.getByText("Activity summary")).toHaveFocus();
  });

  it("waits for a projection at least as new as the recorded revision", () => {
    const first = render(<FocusHarness value={page("2")} />);
    saveFocusedTarget(first.unmount);

    const recreated = render(<FocusHarness value={page("1")} />);
    expect(recreated.getByRole("treeitem")).not.toHaveFocus();
    recreated.rerender(<FocusHarness value={page("2")} />);
    expect(recreated.getByRole("treeitem")).toHaveFocus();
  });

  it("fails closed when session storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("blocked");
    });
    const mounted = render(<FocusHarness value={page("2")} />);
    expect(() => saveFocusedTarget(mounted.unmount)).not.toThrow();

    const recreated = render(<FocusHarness value={page("2")} />);
    expect(recreated.getByRole("treeitem")).not.toHaveFocus();
  });

  it("does not register pagehide when the page is absent", () => {
    const add = vi.spyOn(window, "addEventListener");
    render(<FocusHarness value={null} />);
    expect(add.mock.calls.filter(([type]) => type === "pagehide")).toHaveLength(
      0,
    );
  });

  it("writes the recreation envelope only from the focused surface", () => {
    const add = vi.spyOn(window, "addEventListener");
    const first = render(<DualFocusHarness focusedPage={page("2")} />);
    expect(add.mock.calls.filter(([type]) => type === "pagehide")).toHaveLength(
      1,
    );

    document.querySelector<HTMLElement>('[data-sage-id="item-b"]')!.focus();
    window.dispatchEvent(new Event("pagehide"));
    first.unmount();

    const afterUnfocused = render(<FocusHarness value={page("2")} />);
    expect(afterUnfocused.getByRole("treeitem")).not.toHaveFocus();
    afterUnfocused.unmount();

    const second = render(<DualFocusHarness focusedPage={page("2")} />);
    document.querySelector<HTMLElement>('[data-sage-id="item-a"]')!.focus();
    window.dispatchEvent(new Event("pagehide"));
    second.unmount();

    const afterFocused = render(<FocusHarness value={page("2")} />);
    expect(afterFocused.getByRole("treeitem")).toHaveFocus();
  });
});
