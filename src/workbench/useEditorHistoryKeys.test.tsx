import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as history from "../app/editorHistory";

const apple = vi.hoisted(() => ({ value: false }));
vi.mock("../ui/shortcutLabel", () => ({ usesMetaModifier: () => apple.value }));

import { useEditorHistoryKeys } from "./useEditorHistoryKeys";

function press(init: KeyboardEventInit, target: EventTarget = document.body) {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    ...init,
  });
  target.dispatchEvent(event);
  return event;
}

// Long enough for the history module to have loaded, had a key asked.
const settle = () => new Promise((resolve) => setTimeout(resolve, 50));

beforeEach(() => {
  vi.spyOn(history, "goBack").mockResolvedValue(undefined);
  vi.spyOn(history, "goForward").mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  apple.value = false;
  document.body.innerHTML = "";
});

describe("editor history keys (ED-2b)", () => {
  it("goes back and forward on Alt+Left and Alt+Right, before the editor sees them", async () => {
    renderHook(() => useEditorHistoryKeys(true));
    const editor = document.createElement("div");
    editor.className = "cm-editor";
    const content = document.createElement("div");
    content.setAttribute("contenteditable", "true");
    editor.append(content);
    document.body.append(editor);
    const reachedEditor = vi.fn();
    content.addEventListener("keydown", reachedEditor);

    const back = press({ key: "ArrowLeft", altKey: true }, content);
    expect(back.defaultPrevented).toBe(true);
    expect(reachedEditor).not.toHaveBeenCalled();
    press({ key: "ArrowRight", altKey: true });
    await vi.waitFor(() => {
      expect(history.goBack).toHaveBeenCalledTimes(1);
      expect(history.goForward).toHaveBeenCalledTimes(1);
    });
  });

  it("uses Ctrl+- and Ctrl+Shift+- on macOS, whatever Shift makes of the key", async () => {
    apple.value = true;
    renderHook(() => useEditorHistoryKeys(true));
    expect(press({ key: "ArrowLeft", altKey: true }).defaultPrevented).toBe(
      false,
    );
    press({ key: "-", code: "Minus", ctrlKey: true });
    // On a German layout "-" sits on the Slash key.
    press({ key: "_", code: "Slash", ctrlKey: true, shiftKey: true });
    // Control+ß is on the Minus key there, and is not Go back.
    press({ key: "ß", code: "Minus", ctrlKey: true });
    await vi.waitFor(() => {
      expect(history.goBack).toHaveBeenCalledTimes(1);
      expect(history.goForward).toHaveBeenCalledTimes(1);
    });
  });

  it("ignores the keys while an IME is composing", async () => {
    renderHook(() => useEditorHistoryKeys(true));
    press({ key: "ArrowLeft", altKey: true, isComposing: true });
    await settle();
    expect(history.goBack).not.toHaveBeenCalled();
  });

  it("leaves the chord to a text field or terminal", async () => {
    renderHook(() => useEditorHistoryKeys(true));
    const input = document.createElement("textarea");
    document.body.append(input);
    expect(
      press({ key: "ArrowLeft", altKey: true }, input).defaultPrevented,
    ).toBe(false);
    await settle();
    expect(history.goBack).not.toHaveBeenCalled();
  });

  it("follows the mouse's back and forward buttons", async () => {
    renderHook(() => useEditorHistoryKeys(true));
    window.dispatchEvent(new MouseEvent("mouseup", { button: 3 }));
    window.dispatchEvent(new MouseEvent("mouseup", { button: 4 }));
    window.dispatchEvent(new MouseEvent("mouseup", { button: 0 }));
    await vi.waitFor(() => {
      expect(history.goBack).toHaveBeenCalledTimes(1);
      expect(history.goForward).toHaveBeenCalledTimes(1);
    });
  });

  it("does nothing outside the Editor window", async () => {
    renderHook(() => useEditorHistoryKeys(false));
    press({ key: "ArrowLeft", altKey: true });
    window.dispatchEvent(new MouseEvent("mouseup", { button: 3 }));
    await settle();
    expect(history.goBack).not.toHaveBeenCalled();
  });
});
