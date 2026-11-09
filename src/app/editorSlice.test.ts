import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  EditorDocument,
  ResourceRef,
  WorkbenchTab,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { tabsWithEditorUpdate } from "./editorSlice";
import { useWorkbenchStore } from "./store";

const target: ResourceRef = { rootId: "root-a", relativePath: "typing.ts" };
const other: ResourceRef = { rootId: "root-a", relativePath: "other.ts" };

const fileTab = (resource: ResourceRef, dirty = false): WorkbenchTab => ({
  id: `file:${resource.relativePath}`,
  kind: "file",
  title: resource.relativePath,
  position: 1,
  pinned: false,
  dirty,
  resource,
});

function document(content: string): EditorDocument {
  const version = {
    fingerprint: "saved-0",
    byteLen: content.length,
    modifiedAt: null,
  };
  return {
    resource: target,
    disk: version,
    base: version,
    buffer: {
      fingerprint: version.fingerprint,
      byteLen: content.length,
      revision: 0,
      content,
    },
    lineEnding: "lf",
    dirty: false,
    conflict: null,
  };
}

describe("tabsWithEditorUpdate", () => {
  const tabs = [fileTab(target), fileTab(other, true)];

  it("keeps the same tabs array when nothing on a tab changes", () => {
    expect(tabsWithEditorUpdate(tabs, target)).toBe(tabs);
    expect(tabsWithEditorUpdate(tabs, target, {})).toBe(tabs);
    expect(tabsWithEditorUpdate(tabs, target, { dirty: false })).toBe(tabs);
    expect(tabsWithEditorUpdate(tabs, target, { tabStatus: null })).toBe(tabs);
    expect(tabsWithEditorUpdate(tabs, other, { dirty: true })).toBe(tabs);
  });

  it("replaces only the tab whose dirty flag or status changes", () => {
    const dirty = tabsWithEditorUpdate(tabs, target, { dirty: true });
    expect(dirty).not.toBe(tabs);
    expect(dirty[0]).toMatchObject({ dirty: true });
    expect(dirty[1]).toBe(tabs[1]);

    const failed = tabsWithEditorUpdate(dirty, target, { tabStatus: "failed" });
    expect(failed[0]).toMatchObject({ dirty: true, status: "failed" });
    const cleared = tabsWithEditorUpdate(failed, target, { tabStatus: null });
    expect(cleared[0].status).toBeUndefined();
  });

  it("updates every tab on the resource, such as a Diff opened before the File", () => {
    const diff: WorkbenchTab = {
      ...fileTab(target),
      id: "diff:typing.ts",
      kind: "diff",
    };
    const both = [diff, fileTab(other), fileTab(target)];
    const dirty = tabsWithEditorUpdate(both, target, { dirty: true });
    expect(dirty[0]).toMatchObject({ kind: "diff", dirty: true });
    expect(dirty[1]).toBe(both[1]);
    expect(dirty[2]).toMatchObject({ kind: "file", dirty: true });
  });
});

describe("typing in the active editor", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(
      getWorkbenchPorts().editor,
      "snapshotEditorBuffer",
    ).mockImplementation(async (request) => ({
      revision: request.revision,
      applied: true,
      persistedAt: 1,
      dirty: true,
    }));
    useWorkbenchStore.setState({
      projectTransitioning: false,
      tabs: [fileTab(target), fileTab(other)],
      activeTabId: fileTab(target).id,
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document("base\n"),
        content: "base\n",
        revision: 0,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("changes the tabs list once, on the first dirty keystroke", () => {
    const tabChanges: WorkbenchTab[][] = [];
    const unsubscribe = useWorkbenchStore.subscribe((state, previous) => {
      if (state.tabs !== previous.tabs) tabChanges.push(state.tabs);
    });
    try {
      for (const content of ["base\na", "base\nab", "base\nabc"]) {
        useWorkbenchStore.getState().updateEditorContent(content);
      }
    } finally {
      unsubscribe();
    }

    const state = useWorkbenchStore.getState();
    expect(state.activeEditor).toMatchObject({
      content: "base\nabc",
      revision: 3,
    });
    // Tab strips and the sidebar subscribe to `tabs`; later keystrokes must
    // not hand them a new array.
    expect(tabChanges).toHaveLength(1);
    expect(tabChanges[0][0]).toMatchObject({ dirty: true });
    expect(state.tabs).toBe(tabChanges[0]);
  });
});
