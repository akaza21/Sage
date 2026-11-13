import { cleanup, fireEvent, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { useWorkbenchKeys } from "./useWorkbenchKeys";
import type { WorkbenchWindowMode } from "./workbenchWindowMode";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

function withDirtyFile(windowMode: WorkbenchWindowMode) {
  useWorkbenchStore.setState({
    activeTabId: "file-a",
    tabs: [
      {
        id: "file-a",
        kind: "file",
        title: "main.ts",
        position: 0,
        pinned: false,
        dirty: true,
        resource: { rootId: "root-a", relativePath: "src/main.ts" },
      },
    ],
  });
  renderHook(() =>
    useWorkbenchKeys({
      resourceModal: false,
      windowMode,
      toggleAgentNavigation: () => {},
      setWindowMode: () => {},
    }),
  );
}

it("asks before Ctrl+W closes a dirty editor tab", () => {
  withDirtyFile("editor");

  fireEvent.keyDown(window, { key: "w", ctrlKey: true });

  expect(useWorkbenchStore.getState().pendingCloseTabId).toBe("file-a");
  expect(useWorkbenchStore.getState().tabs).toHaveLength(1);
});

it("leaves Ctrl+W alone in Agent mode", () => {
  withDirtyFile("agent");

  fireEvent.keyDown(window, { key: "w", ctrlKey: true });

  expect(useWorkbenchStore.getState().pendingCloseTabId).toBeNull();
});
