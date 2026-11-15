import { Text } from "@codemirror/state";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ResourceRef, WorkbenchTab } from "../domain/workbench";
import type { FileLocationNavigationRequest } from "./fileNavigationSlice";
import { useEditorCaretStore } from "./editorCaretStore";
import {
  MAX_EDITOR_PLACES,
  canGoBack,
  canGoForward,
  goBack,
  goForward,
  notePlace,
  resetEditorHistory,
  watchEditorHistory,
} from "./editorHistory";
import { useWorkbenchStore, type WorkbenchState } from "./store";

const initialState = useWorkbenchStore.getState();
const a: ResourceRef = { rootId: "root", relativePath: "src/a.ts" };
const b: ResourceRef = { rootId: "root", relativePath: "src/b.ts" };
const c: ResourceRef = { rootId: "root", relativePath: "src/c.ts" };

const place = (resource: ResourceRef, line: number, column = 1) => ({
  projectId: "project",
  resource,
  line,
  column,
});

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

function showFile(resource: ResourceRef, phase = "ready") {
  useWorkbenchStore.setState({
    tabs: [fileTab(resource)],
    activeTabId: fileTab(resource).id,
    activeEditor: {
      phase,
      resource,
    } as unknown as WorkbenchState["activeEditor"],
  });
}

const tallText = Text.of(Array.from({ length: 400 }, () => "xx"));
const caretAt = (line: number, editable = true) =>
  useEditorCaretStore.getState().publish(tallText, (line - 1) * 3, editable);

let opened: FileLocationNavigationRequest[] = [];
let openable = true;
let readable = true;

beforeEach(() => {
  opened = [];
  openable = true;
  readable = true;
  useWorkbenchStore.setState({
    project: { id: "project" } as WorkbenchState["project"],
    nativeTransitionEpoch: 4,
    openFileLocation: vi.fn(async (request: FileLocationNavigationRequest) => {
      opened.push(request);
      if (!openable) return;
      showFile(request.resource, readable ? "ready" : "error");
      // Like the real store: the target waits for the editor to apply it,
      // and is set even when the File failed to load.
      useWorkbenchStore.setState({
        fileNavigationTarget: { ...request, projectGeneration: 1 },
      });
    }),
  });
});

afterEach(() => {
  resetEditorHistory();
  useEditorCaretStore.getState().clear();
  useWorkbenchStore.setState(initialState, true);
});

describe("editor history (ED-2b)", () => {
  it("goes back to the place in the File left behind, and forward again", async () => {
    notePlace(place(a, 1));
    notePlace(place(a, 40, 7));
    notePlace(place(b, 1));
    expect(canGoBack()).toBe(true);
    expect(canGoForward()).toBe(false);

    await goBack();
    expect(opened.at(-1)).toMatchObject({
      projectId: "project",
      transitionEpoch: 4,
      resource: a,
      lineNumber: 40,
      columnNumber: 7,
      selection: { kind: "caret" },
    });
    expect(canGoForward()).toBe(true);

    await goForward();
    expect(opened.at(-1)).toMatchObject({ resource: b, lineNumber: 1 });
    expect(canGoForward()).toBe(false);
  });

  it("follows small caret moves and leaves a place on a far one", async () => {
    notePlace(place(a, 1));
    notePlace(place(a, 5));
    notePlace(place(a, 12));
    notePlace(place(a, 20));
    expect(canGoBack()).toBe(false);
    notePlace(place(a, 300));
    await goBack();
    expect(opened.at(-1)).toMatchObject({ resource: a, lineNumber: 20 });
  });

  it("lets the jump that opened a File replace where it opened", async () => {
    notePlace(place(a, 1));
    notePlace(place(a, 50));
    notePlace(place(b, 1));
    notePlace(place(b, 200));
    await goBack();
    expect(opened.at(-1)).toMatchObject({ resource: a, lineNumber: 50 });
    expect(canGoBack()).toBe(false);
  });

  it("drops the forward places when a new place is left", async () => {
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    await goBack();
    expect(canGoForward()).toBe(true);
    notePlace(place(c, 1));
    expect(canGoForward()).toBe(false);
  });

  it("keeps the latest places only", async () => {
    for (let index = 0; index <= MAX_EDITOR_PLACES + 5; index += 1) {
      notePlace(place({ rootId: "root", relativePath: `f${index}.ts` }, 1));
    }
    let steps = 0;
    while (canGoBack()) {
      await goBack();
      steps += 1;
    }
    expect(steps).toBe(MAX_EDITOR_PLACES);
    expect(opened.at(-1)?.resource.relativePath).toBe("f5.ts");
  });

  it("forgets the places of another Project", () => {
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    notePlace({ ...place(c, 1), projectId: "other" });
    expect(canGoBack()).toBe(false);
    useWorkbenchStore.setState({
      project: { id: "other" } as WorkbenchState["project"],
    });
    expect(canGoBack()).toBe(false);
  });

  it("offers nothing for a Project that is no longer open", () => {
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    useWorkbenchStore.setState({ project: null });
    expect(canGoBack()).toBe(false);
  });

  it("skips a place that can no longer be opened", async () => {
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    notePlace(place(c, 1));
    showFile(c);
    openable = false;
    await goBack();
    expect(opened.at(-1)?.resource).toEqual(b);
    expect(canGoForward()).toBe(false);
    openable = true;
    await goBack();
    expect(opened.at(-1)?.resource).toEqual(a);
    await goForward();
    expect(opened.at(-1)?.resource).toEqual(c);
  });

  it("notes the active File's caret once watching", async () => {
    watchEditorHistory();
    const caret = useEditorCaretStore.getState();
    showFile(a);
    caret.publish(Text.of(Array.from({ length: 100 }, () => "x")), 0);
    caret.publish(Text.of(Array.from({ length: 100 }, () => "x")), 2 * 60);
    showFile(b);
    caret.publish(Text.of(["one", "two"]), 0);
    expect(canGoBack()).toBe(true);
    await goBack();
    expect(opened.at(-1)).toMatchObject({ resource: a, lineNumber: 61 });
  });

  it("does not note a caret outside an editable File", () => {
    watchEditorHistory();
    useWorkbenchStore.setState({ tabs: [], activeTabId: null });
    useEditorCaretStore.getState().publish(Text.of(["x", "y"]), 0);
    showFile(a);
    useEditorCaretStore.getState().publish(Text.of(["x", "y"]), 0);
    expect(canGoBack()).toBe(false);
  });

  it("adds nothing while a Go Back arrives, then follows the caret again", async () => {
    watchEditorHistory();
    showFile(a);
    caretAt(1);
    caretAt(200);
    showFile(b);
    useEditorCaretStore.getState().clear();
    caretAt(1);
    caretAt(5);
    await goBack();
    expect(opened.at(-1)).toMatchObject({ resource: a, lineNumber: 200 });
    useEditorCaretStore.getState().clear();
    caretAt(1);
    caretAt(200);
    useWorkbenchStore.setState({ fileNavigationTarget: null });
    caretAt(203);
    expect(canGoBack()).toBe(false);
    expect(canGoForward()).toBe(true);
    await goForward();
    expect(opened.at(-1)).toMatchObject({ resource: b, lineNumber: 5 });
  });

  it("lets a later Go Back go when a target was never applied", async () => {
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    notePlace(place(c, 1));
    await goBack();
    await goBack();
    expect(opened.at(-1)?.resource).toEqual(a);
    expect(canGoForward()).toBe(true);
  });

  it("drops a place whose File can't be read, so the next Go Back moves on", async () => {
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    notePlace(place(c, 1));
    readable = false;
    await goBack();
    expect(opened.at(-1)?.resource).toEqual(b);
    readable = true;
    await goBack();
    expect(opened.at(-1)?.resource).toEqual(a);
    await goForward();
    expect(opened.at(-1)?.resource).toEqual(c);
  });

  it("keeps the place when the Editor is busy for a moment", async () => {
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    openable = false;
    useWorkbenchStore.setState({ fileRenameInFlight: true });
    await goBack();
    expect(canGoBack()).toBe(true);
    expect(canGoForward()).toBe(false);
  });

  it("ignores the top-of-File caret of an editor coming back from another tab", async () => {
    watchEditorHistory();
    showFile(a);
    caretAt(1);
    caretAt(2);
    caretAt(60);
    useEditorCaretStore.getState().clear();
    caretAt(1);
    expect(canGoBack()).toBe(true);
    await goBack();
    expect(opened.at(-1)).toMatchObject({ resource: a, lineNumber: 2 });
  });

  it("does not note a read-only File's caret", () => {
    watchEditorHistory();
    showFile(a);
    caretAt(1, false);
    showFile(b);
    caretAt(1, false);
    expect(canGoBack()).toBe(false);
  });

  it("keeps Go Forward when the File got shorter and the caret lands elsewhere", async () => {
    watchEditorHistory();
    showFile(a);
    caretAt(1);
    caretAt(200);
    showFile(b);
    useEditorCaretStore.getState().clear();
    caretAt(1);
    await goBack();
    useEditorCaretStore.getState().clear();
    caretAt(1);
    caretAt(50);
    useWorkbenchStore.setState({ fileNavigationTarget: null });
    expect(canGoBack()).toBe(false);
    expect(canGoForward()).toBe(true);
  });

  it("ends a Go Back on the first move when nothing applies its target", async () => {
    watchEditorHistory();
    showFile(a);
    caretAt(1);
    caretAt(5);
    showFile(b);
    useEditorCaretStore.getState().clear();
    caretAt(1);
    caretAt(2);
    await goBack();
    useEditorCaretStore.getState().clear();
    caretAt(1);
    caretAt(3);
    caretAt(300);
    expect(canGoBack()).toBe(true);
    await goBack();
    expect(opened.at(-1)).toMatchObject({ resource: a, lineNumber: 3 });
  });

  it("stays put when a Go Back in the same File is refused for a moment", async () => {
    notePlace(place(a, 1));
    notePlace(place(a, 2));
    notePlace(place(a, 300));
    showFile(a);
    openable = false;
    useWorkbenchStore.setState({ fileCreateInFlight: true });
    await goBack();
    notePlace(place(a, 301));
    useWorkbenchStore.setState({ fileCreateInFlight: false });
    openable = true;
    await goBack();
    expect(opened.at(-1)).toMatchObject({ resource: a, lineNumber: 2 });
    expect(canGoBack()).toBe(false);
  });

  it("starts afresh after a place that could not be opened", async () => {
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    readable = false;
    await goBack();
    readable = true;
    expect(canGoForward()).toBe(true);
    notePlace(place(c, 1));
    expect(canGoForward()).toBe(false);
    notePlace({ ...place(a, 1), projectId: "other" });
    useWorkbenchStore.setState({
      project: { id: "other" } as WorkbenchState["project"],
    });
    expect(canGoBack()).toBe(false);
  });

  it("closes the tab it opened for a File that is gone, and keeps one that was open", async () => {
    const closeTab = vi.fn();
    useWorkbenchStore.setState({ closeTab });
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    notePlace(place(c, 1));
    readable = false;
    await goBack();
    expect(closeTab).toHaveBeenCalledWith(fileTab(b).id);
    closeTab.mockClear();
    showFile(a, "error");
    await goBack();
    expect(closeTab).not.toHaveBeenCalled();
  });

  it("ignores the left File's carets while the next one opens", async () => {
    notePlace(place(a, 1));
    notePlace(place(b, 1));
    useWorkbenchStore.setState({
      openFileLocation: vi.fn(
        async (request: FileLocationNavigationRequest) => {
          opened.push(request);
          notePlace(place(b, 2));
          showFile(request.resource);
          useWorkbenchStore.setState({
            fileNavigationTarget: { ...request, projectGeneration: 1 },
          });
        },
      ),
    });
    await goBack();
    expect(canGoBack()).toBe(false);
    expect(canGoForward()).toBe(true);
  });

  it("finishes a Go Back whose target was taken before the open returned", async () => {
    watchEditorHistory();
    showFile(a);
    caretAt(1);
    caretAt(200);
    showFile(b);
    useEditorCaretStore.getState().clear();
    caretAt(1);
    // As in the app: the editor takes the target before the open returns,
    // and its carets follow.
    useWorkbenchStore.setState({
      openFileLocation: vi.fn(
        async (request: FileLocationNavigationRequest) => {
          opened.push(request);
          showFile(request.resource);
          useWorkbenchStore.setState({ fileNavigationTarget: null });
        },
      ),
    });
    await goBack();
    useEditorCaretStore.getState().clear();
    caretAt(1);
    caretAt(200);
    expect(canGoBack()).toBe(false);
    expect(canGoForward()).toBe(true);
    caretAt(400);
    expect(canGoBack()).toBe(true);
  });
});
