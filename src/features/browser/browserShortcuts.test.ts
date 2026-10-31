import { describe, expect, it, vi } from "vitest";
import { dispatchBrowserChromeShortcut } from "./browserShortcuts";

function press(
  root: HTMLElement,
  target: HTMLElement,
  init: KeyboardEventInit,
) {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    ...init,
  });
  target.dispatchEvent(event);
  return event;
}

describe("browser chrome shortcuts", () => {
  it("routes the admitted keys when focus is inside the browser pane", () => {
    const root = document.createElement("section");
    const address = document.createElement("input");
    root.append(address);
    const dispatch = vi.fn();
    root.addEventListener("keydown", (event) =>
      dispatchBrowserChromeShortcut(root, event, true, dispatch),
    );
    for (const [init, action] of [
      [{ key: "l", ctrlKey: true }, "focusAddress"],
      [{ key: "F5" }, "reload"],
      [{ key: "r", ctrlKey: true }, "reload"],
      [{ key: "ArrowLeft", altKey: true }, "back"],
      [{ key: "ArrowRight", altKey: true }, "forward"],
      [{ key: "f", ctrlKey: true }, "find"],
      [{ key: "=", code: "Equal", ctrlKey: true }, "zoomIn"],
      [{ key: "+", ctrlKey: true, shiftKey: true }, "zoomIn"],
      [{ key: "=", code: "Equal", ctrlKey: true, shiftKey: true }, "zoomIn"],
      [{ key: "+", code: "NumpadAdd", ctrlKey: true }, "zoomIn"],
      [{ key: "Add", code: "NumpadAdd", ctrlKey: true }, "zoomIn"],
      [{ key: "-", ctrlKey: true }, "zoomOut"],
      [{ key: "0", ctrlKey: true }, "zoomReset"],
      [{ key: "t", ctrlKey: true }, "newTab"],
      [{ key: "w", ctrlKey: true }, "closeTab"],
      [{ key: "h", ctrlKey: true }, "history"],
      [{ key: "Escape" }, "stop"],
    ] as const) {
      const event = press(root, address, init);
      expect(event.defaultPrevented).toBe(true);
      expect(dispatch).toHaveBeenLastCalledWith(action);
    }
  });

  it("leaves unrelated typing and composer shortcuts alone", () => {
    const root = document.createElement("section");
    const address = document.createElement("input");
    root.append(address);
    const composer = document.createElement("textarea");
    const dispatch = vi.fn();
    const events = [
      press(root, address, { key: "l" }),
      press(root, address, { key: "Escape" }),
      press(root, composer, { key: "l", ctrlKey: true }),
      press(root, address, { key: "t", ctrlKey: true, shiftKey: true }),
      press(root, address, { key: "=", code: "Equal" }),
      press(root, address, { key: "=", ctrlKey: true, altKey: true }),
      press(root, address, { key: "=", ctrlKey: true, metaKey: true }),
    ];
    for (const event of events) {
      dispatchBrowserChromeShortcut(root, event, false, dispatch);
      expect(event.defaultPrevented).toBe(false);
    }
    expect(dispatch).not.toHaveBeenCalled();
  });
});
