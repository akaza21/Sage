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
  value: AcpChatPage;
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
});
