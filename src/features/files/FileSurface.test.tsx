import { act, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { EditorDocument, WorkbenchTab } from "../../domain/workbench";
import { desktop } from "../../services/desktop";
import { FileSurface } from "./FileSurface";

const path = "debounce.txt";
const resource = { rootId: "root-a", relativePath: path };
const version = { fingerprint: "base", byteLen: 5, modifiedAt: null };
const document: EditorDocument = {
  resource,
  disk: version,
  base: version,
  buffer: {
    fingerprint: "base",
    byteLen: 5,
    revision: 0,
    content: "base\n",
  },
  lineEnding: "lf",
  dirty: false,
  conflict: null,
};
const tab: WorkbenchTab = {
  id: `file:${path}`,
  kind: "file",
  title: path,
  position: 1,
  pinned: false,
  dirty: false,
  resource,
};

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("FileSurface durability cadence", () => {
  it("sends one trailing snapshot after 750 ms", async () => {
    vi.useFakeTimers();
    const snapshot = vi
      .spyOn(desktop, "snapshotEditorBuffer")
      .mockResolvedValue({ revision: 1, persistedAt: 1, dirty: true });
    useWorkbenchStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
      activeEditor: {
        phase: "ready",
        resource,
        document,
        content: document.buffer.content,
        revision: 0,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });
    render(<FileSurface tab={tab} />);

    act(() => {
      useWorkbenchStore.getState().updateEditorContent("edited\n");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(749);
    });
    expect(snapshot).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(snapshot).toHaveBeenCalledTimes(1);
    expect(snapshot).toHaveBeenCalledWith(
      expect.objectContaining({ resource, content: "edited\n", revision: 1 }),
    );
  });
});
