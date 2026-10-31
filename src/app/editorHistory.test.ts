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

let opened: FileLocationNavigationRequest[] = [];
let openable = true;

beforeEach(() => {
  opened = [];
  openable = true;
  useWorkbenchStore.setState({
    project: { id: "project" } as WorkbenchState["project"],
    nativeTransitionEpoch: 4,
    openFileLocation: vi.fn(async (request: FileLocationNavigationRequest) => {
      opened.push(request);
      if (openable) showFile(request.resource);
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
    notePlace(place(a, 5)); // the first move settles where a opened
    notePlace(place(a, 12));
    notePlace(place(a, 20)); // within 10 lines: the same place, moved
    expect(canGoBack()).toBe(false);
    notePlace(place(a, 300));
    await goBack();
    expect(opened.at(-1)).toMatchObject({ resource: a, lineNumber: 20 });
  });

  it("lets the jump that opened a File replace where it opened", async () => {
    notePlace(place(a, 1));
    notePlace(place(a, 50));
    notePlace(place(b, 1)); // b opens at its start...
    notePlace(place(b, 200)); // ...then Quick Open's :200 moves there
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
});
