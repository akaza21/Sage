import { fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  installTooltipLayer,
  TOOLTIP_DELAY_MS,
  TOOLTIP_HIDE_GRACE_MS,
  TOOLTIP_SKIP_DELAY_MS,
} from "./tooltipLayer";

let uninstall: () => void;
let resized: (() => void) | undefined;

function rect(left: number, top: number, width: number, height: number) {
  return DOMRect.fromRect({ x: left, y: top, width, height });
}

beforeEach(() => {
  vi.useFakeTimers();
  // jsdom has no layout: a connected, unhidden element has one box.
  vi.spyOn(Element.prototype, "getClientRects").mockImplementation(function (
    this: Element,
  ) {
    const boxed = this.isConnected && !this.closest("[hidden]");
    return (boxed ? [rect(0, 100, 24, 24)] : []) as unknown as DOMRectList;
  });
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        resized = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  document.body.innerHTML = `
    <div id="pane">
      <button data-tooltip="Attach files"><svg></svg></button>
      <button class="icon-button" aria-label="New terminal"></button>
      <button class="icon-button" aria-label="Options" title="No options"></button>
      <button>Plain</button>
    </div>`;
  uninstall = installTooltipLayer();
});

afterEach(() => {
  uninstall();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

const button = (name: string) =>
  [...document.querySelectorAll("button")].find(
    (element) =>
      (element.dataset.tooltip ??
        element.getAttribute("aria-label") ??
        element.textContent) === name,
  )!;
const tooltip = () => document.querySelector<HTMLElement>('[role="tooltip"]')!;
const visible = () => (tooltip().hidden ? null : tooltip().textContent);

function hoverShown(name: string) {
  fireEvent.pointerOver(button(name));
  vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
}

describe("installTooltipLayer", () => {
  it("shows after the delay on hover, including over the control's icon", () => {
    fireEvent.pointerOver(button("Attach files"));
    vi.advanceTimersByTime(TOOLTIP_DELAY_MS - 1);
    fireEvent.pointerOver(button("Attach files").querySelector("svg")!);
    expect(visible()).toBeNull();
    vi.advanceTimersByTime(1);
    expect(visible()).toBe("Attach files");
    expect(tooltip()).toHaveAttribute("aria-hidden", "true");
  });

  it("ignores touch", () => {
    fireEvent.pointerOver(button("Attach files"), { pointerType: "touch" });
    vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
    expect(visible()).toBeNull();
  });

  it("shows the next control at once after one was shown, then cools down", () => {
    hoverShown("Attach files");
    fireEvent.pointerOver(button("New terminal"));
    expect(visible()).toBe("New terminal");
    fireEvent.pointerDown(button("New terminal"));
    expect(visible()).toBeNull();
    vi.advanceTimersByTime(TOOLTIP_SKIP_DELAY_MS);
    fireEvent.pointerOver(button("Attach files"));
    expect(visible()).toBeNull();
  });

  it("stays while the pointer moves onto it, and hides after leaving both", () => {
    hoverShown("Attach files");
    fireEvent.pointerOut(button("Attach files"), { relatedTarget: tooltip() });
    fireEvent.pointerOver(tooltip());
    vi.advanceTimersByTime(TOOLTIP_HIDE_GRACE_MS);
    expect(visible()).toBe("Attach files");
    fireEvent.pointerOut(tooltip(), { relatedTarget: button("Plain") });
    vi.advanceTimersByTime(TOOLTIP_HIDE_GRACE_MS);
    expect(visible()).toBeNull();
  });

  it("hides when the pointer leaves the window, and cancels a pending one", () => {
    hoverShown("Attach files");
    fireEvent.pointerOut(button("Attach files"), { relatedTarget: null });
    vi.advanceTimersByTime(TOOLTIP_HIDE_GRACE_MS);
    expect(visible()).toBeNull();
    vi.advanceTimersByTime(TOOLTIP_SKIP_DELAY_MS);

    fireEvent.pointerOver(button("Attach files"));
    fireEvent.pointerOut(button("Attach files"), { relatedTarget: null });
    vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
    expect(visible()).toBeNull();
  });

  it("leaves an icon button with a native title to that title", () => {
    hoverShown("Options");
    expect(visible()).toBeNull();
  });

  it("shows at once on keyboard focus but not on pointer focus", () => {
    const attach = button("Attach files");
    const matches = vi.spyOn(attach, "matches");
    matches.mockReturnValue(false);
    fireEvent.focusIn(attach);
    expect(visible()).toBeNull();
    matches.mockReturnValue(true);
    fireEvent.focusIn(attach);
    expect(visible()).toBe("Attach files");
    fireEvent.focusOut(attach);
    expect(visible()).toBeNull();
  });

  it("keeps a hover tooltip when focus moves to something without one", () => {
    hoverShown("Attach files");
    fireEvent.focusIn(button("Plain"));
    expect(visible()).toBe("Attach files");
  });

  it("hides on any key, without taking Escape from the dialog around it", () => {
    hoverShown("Attach files");
    const pane = vi.fn();
    document.getElementById("pane")!.addEventListener("keydown", pane);
    fireEvent.keyDown(button("Attach files"), { key: "Escape" });
    expect(visible()).toBeNull();
    expect(pane).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(TOOLTIP_SKIP_DELAY_MS);
    hoverShown("Attach files");
    fireEvent.keyDown(button("Attach files"), { key: "Enter" });
    expect(visible()).toBeNull();
  });

  it("hides when the control's own container scrolls, not for other scrolls", () => {
    const elsewhere = document.createElement("div");
    document.body.append(elsewhere);
    hoverShown("Attach files");
    fireEvent.scroll(elsewhere);
    expect(visible()).toBe("Attach files");
    fireEvent.scroll(document.getElementById("pane")!);
    expect(visible()).toBeNull();
  });

  it("hides when the shown control is hidden or removed", () => {
    hoverShown("Attach files");
    document.getElementById("pane")!.hidden = true;
    resized?.();
    expect(visible()).toBeNull();
  });

  it("does not show a control that was removed before the delay ended", () => {
    fireEvent.pointerOver(button("Attach files"));
    button("Attach files").remove();
    vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
    expect(visible()).toBeNull();
  });

  it("renders inside a modal dialog so the dialog does not cover it", () => {
    const dialog = document.createElement("dialog");
    dialog.innerHTML = `<button data-tooltip="Close dialog"></button>`;
    document.body.append(dialog);
    hoverShown("Close dialog");
    expect(tooltip().parentElement).toBe(dialog);
    expect(visible()).toBe("Close dialog");
  });

  it("sits above the control, below it near the top edge, inside the window", () => {
    vi.spyOn(tooltip(), "getBoundingClientRect").mockReturnValue(
      rect(0, 0, 100, 20),
    );
    const attach = button("Attach files");
    const anchor = vi.spyOn(attach, "getBoundingClientRect");
    anchor.mockReturnValue(rect(200, 300, 24, 24));
    hoverShown("Attach files");
    expect(tooltip().style.left).toBe("162px");
    expect(tooltip().style.top).toBe("274px");

    fireEvent.pointerDown(attach);
    anchor.mockReturnValue(rect(2, 4, 24, 24));
    hoverShown("Attach files");
    expect(tooltip().style.left).toBe("8px");
    expect(tooltip().style.top).toBe("34px");
  });

  it("removes its node and listeners when uninstalled", () => {
    uninstall();
    expect(document.querySelector('[role="tooltip"]')).toBeNull();
    hoverShown("Attach files");
    expect(document.querySelector('[role="tooltip"]')).toBeNull();
    uninstall = installTooltipLayer();
  });
});
