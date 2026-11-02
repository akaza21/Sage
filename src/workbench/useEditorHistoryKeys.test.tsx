import { cleanup, renderHook } from "@testing-library/react";
import { Terminal } from "@xterm/xterm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { notePlace, resetEditorHistory } from "../app/editorHistory";
import type { FileLocationNavigationRequest } from "../app/fileNavigationSlice";
import { useWorkbenchStore, type WorkbenchState } from "../app/store";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";
import { settleLazyModules } from "../test/settleLazyModules";

const apple = vi.hoisted(() => ({ value: false }));
vi.mock("../ui/shortcutLabel", () => ({ usesMetaModifier: () => apple.value }));

import { useEditorHistoryKeys } from "./useEditorHistoryKeys";

const initialState = useWorkbenchStore.getState();
const a: ResourceRef = { rootId: "root", relativePath: "src/a.ts" };
const b: ResourceRef = { rootId: "root", relativePath: "src/b.ts" };
const c: ResourceRef = { rootId: "root", relativePath: "src/c.ts" };

function fileTab(resource: ResourceRef): WorkbenchTab {
  return {
    id: `file:${resource.relativePath}`,
    kind: "file",
    title: resource.relativePath,
    position: 0,
    pinned: false,
    dirty: false,
    resource,
  };
}

function showFile(resource: ResourceRef) {
  useWorkbenchStore.setState({
    tabs: [fileTab(resource)],
    activeTabId: fileTab(resource).id,
    activeEditor: {
      phase: "ready",
      resource,
    } as unknown as WorkbenchState["activeEditor"],
  });
}

/** The active File and the caret the editor was asked to put there. */
function where() {
  const state = useWorkbenchStore.getState();
  const tab = state.tabs.find((item) => item.id === state.activeTabId);
  const target = state.fileNavigationTarget;
  return {
    file: tab?.resource?.relativePath,
    caret: target ? `${target.lineNumber}:${target.columnNumber}` : null,
  };
}

function press(init: KeyboardEventInit, target: EventTarget = document.body) {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    ...init,
  });
  target.dispatchEvent(event);
  return event;
}

const altLeft = { key: "ArrowLeft", code: "ArrowLeft", altKey: true };
const altRight = { key: "ArrowRight", code: "ArrowRight", altKey: true };

const atStart = { file: "src/c.ts", caret: null };
const oneBack = { file: "src/b.ts", caret: "1:1" };
const twoBack = { file: "src/a.ts", caret: "40:7" };

/**
 * A trip starts once the history module's dynamic import settles; after
 * that, the Editor is still where it was only if nothing asked to travel.
 * The history is left whole: one Go back then lands on b, not a.
 */
async function expectNoTrip() {
  await settleLazyModules();
  expect(where()).toEqual(atStart);
  press(altLeft);
  await vi.waitFor(() => expect(where()).toEqual(oneBack));
}

beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project" } as WorkbenchState["project"],
    nativeTransitionEpoch: 4,
    // Opening a File makes it active and asks its editor for the caret, as
    // the store does once the document has loaded.
    openFileLocation: async (request: FileLocationNavigationRequest) => {
      showFile(request.resource);
      useWorkbenchStore.setState({
        fileNavigationTarget: { ...request, projectGeneration: 1 },
      });
    },
  });
  // Visited: a (caret at 40:7), then b, and now c.
  notePlace({ projectId: "project", resource: a, line: 40, column: 7 });
  notePlace({ projectId: "project", resource: b, line: 1, column: 1 });
  notePlace({ projectId: "project", resource: c, line: 1, column: 1 });
  showFile(c);
});

afterEach(() => {
  cleanup();
  apple.value = false;
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
  resetEditorHistory();
  useWorkbenchStore.setState(initialState, true);
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

    const back = press(altLeft, content);
    expect(back.defaultPrevented).toBe(true);
    expect(reachedEditor).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(where()).toEqual(oneBack));

    press(altLeft, content);
    await vi.waitFor(() => expect(where()).toEqual(twoBack));

    expect(press(altRight, content).defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(where()).toEqual(oneBack));
    expect(reachedEditor).not.toHaveBeenCalled();
  });

  it("uses Ctrl+- and Ctrl+Shift+- on macOS, whatever Shift makes of the key", async () => {
    apple.value = true;
    renderHook(() => useEditorHistoryKeys(true));

    expect(press(altLeft).defaultPrevented).toBe(false);
    // Control+ß is on the Minus key of a German layout, and is not Go back.
    expect(
      press({ key: "ß", code: "Minus", ctrlKey: true }).defaultPrevented,
    ).toBe(false);
    await settleLazyModules();
    expect(where()).toEqual(atStart);

    expect(
      press({ key: "-", code: "Minus", ctrlKey: true }).defaultPrevented,
    ).toBe(true);
    await vi.waitFor(() => expect(where()).toEqual(oneBack));

    // On a German layout "-" sits on the Slash key.
    press({ key: "_", code: "Slash", ctrlKey: true, shiftKey: true });
    await vi.waitFor(() =>
      expect(where()).toEqual({ file: "src/c.ts", caret: "1:1" }),
    );
  });

  it("ignores the keys while an IME is composing", async () => {
    renderHook(() => useEditorHistoryKeys(true));

    expect(press({ ...altLeft, isComposing: true }).defaultPrevented).toBe(
      false,
    );
    await expectNoTrip();
  });

  it("leaves the chord to a text field", async () => {
    renderHook(() => useEditorHistoryKeys(true));
    const field = document.createElement("textarea");
    document.body.append(field);
    const reachedField = vi.fn();
    field.addEventListener("keydown", reachedField);

    expect(press(altLeft, field).defaultPrevented).toBe(false);
    expect(reachedField).toHaveBeenCalledOnce();
    await expectNoTrip();
  });

  it("leaves the chord to the terminal, which sends it to the shell", async () => {
    // xterm asks for the display's pixel ratio, which jsdom does not model.
    vi.stubGlobal(
      "matchMedia",
      (media: string) =>
        ({
          matches: false,
          media,
          addListener: () => undefined,
          removeListener: () => undefined,
        }) as unknown as MediaQueryList,
    );
    renderHook(() => useEditorHistoryKeys(true));
    const host = document.createElement("div");
    document.body.append(host);
    const terminal = new Terminal();
    terminal.open(host);
    const sent: string[] = [];
    terminal.onData((data) => sent.push(data));

    press({ ...altLeft, keyCode: 37 }, terminal.textarea!);
    // The shell gets its word-left sequence.
    expect(sent).toEqual(["\u001b[1;3D"]);
    await expectNoTrip();

    terminal.dispose();
  });

  it("follows the mouse's back and forward buttons", async () => {
    renderHook(() => useEditorHistoryKeys(true));

    window.dispatchEvent(new MouseEvent("mouseup", { button: 3 }));
    await vi.waitFor(() => expect(where()).toEqual(oneBack));
    // The primary button is neither.
    window.dispatchEvent(new MouseEvent("mouseup", { button: 0 }));
    await settleLazyModules();
    expect(where()).toEqual(oneBack);

    window.dispatchEvent(new MouseEvent("mouseup", { button: 3 }));
    await vi.waitFor(() => expect(where()).toEqual(twoBack));
    window.dispatchEvent(new MouseEvent("mouseup", { button: 4 }));
    await vi.waitFor(() => expect(where()).toEqual(oneBack));
  });

  it("does nothing outside the Editor window", async () => {
    const { rerender } = renderHook(
      ({ editorWindow }) => useEditorHistoryKeys(editorWindow),
      { initialProps: { editorWindow: false } },
    );

    expect(press(altLeft).defaultPrevented).toBe(false);
    window.dispatchEvent(new MouseEvent("mouseup", { button: 3 }));
    await settleLazyModules();
    expect(where()).toEqual(atStart);

    rerender({ editorWindow: true });
    await expectNoTrip();
  });
});
