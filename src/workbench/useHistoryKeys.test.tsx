import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as history from "../app/editorHistory";
import * as chatHistory from "./agentChatHistory";

const apple = vi.hoisted(() => ({ value: false }));
vi.mock("../ui/shortcutLabel", () => ({ usesMetaModifier: () => apple.value }));

import { useHistoryKeys } from "./useHistoryKeys";

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
  vi.spyOn(chatHistory, "goBackToPreviousChat").mockResolvedValue(undefined);
  vi.spyOn(chatHistory, "goForwardToNextChat").mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  apple.value = false;
  document.body.innerHTML = "";
});

describe("history keys", () => {
  it("goes back and forward on Alt+Left and Alt+Right, before the editor sees them", async () => {
    renderHook(() => useHistoryKeys("editor"));
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
    renderHook(() => useHistoryKeys("editor"));
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
    renderHook(() => useHistoryKeys("editor"));
    press({ key: "ArrowLeft", altKey: true, isComposing: true });
    await settle();
    expect(history.goBack).not.toHaveBeenCalled();
  });

  it("leaves the chord to a text field or terminal", async () => {
    renderHook(() => useHistoryKeys("editor"));
    const input = document.createElement("textarea");
    document.body.append(input);
    expect(
      press({ key: "ArrowLeft", altKey: true }, input).defaultPrevented,
    ).toBe(false);
    await settle();
    expect(history.goBack).not.toHaveBeenCalled();
  });

  it("follows the mouse's back and forward buttons", async () => {
    renderHook(() => useHistoryKeys("editor"));
    window.dispatchEvent(new MouseEvent("mouseup", { button: 3 }));
    window.dispatchEvent(new MouseEvent("mouseup", { button: 4 }));
    window.dispatchEvent(new MouseEvent("mouseup", { button: 0 }));
    await vi.waitFor(() => {
      expect(history.goBack).toHaveBeenCalledTimes(1);
      expect(history.goForward).toHaveBeenCalledTimes(1);
    });
  });

  it("travels through chats, not File places, in the Agent window", async () => {
    renderHook(() => useHistoryKeys("agent"));
    press({ key: "ArrowLeft", altKey: true });
    window.dispatchEvent(new MouseEvent("mouseup", { button: 4 }));
    await settle();
    expect(chatHistory.goBackToPreviousChat).toHaveBeenCalledTimes(1);
    expect(chatHistory.goForwardToNextChat).toHaveBeenCalledTimes(1);
    expect(history.goBack).not.toHaveBeenCalled();
    expect(history.goForward).not.toHaveBeenCalled();
  });

  it("does nothing while Settings covers the window", async () => {
    renderHook(() => useHistoryKeys(null));
    press({ key: "ArrowLeft", altKey: true });
    window.dispatchEvent(new MouseEvent("mouseup", { button: 3 }));
    await settle();
    expect(history.goBack).not.toHaveBeenCalled();
  });
});
