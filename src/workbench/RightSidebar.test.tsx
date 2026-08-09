import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { workspaceSearchDesktop } from "../features/search";
import { RightSidebar } from "./RightSidebar";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

describe("RightSidebar Search", () => {
  it("searches the active Project and opens a result through the workbench", async () => {
    const openFile = vi.fn(async () => undefined);
    const search = vi
      .spyOn(workspaceSearchDesktop, "search")
      .mockResolvedValue({
        requestId: "request-a",
        projectId: "project-a",
        transitionEpoch: 7,
        query: "needle",
        matches: [
          {
            resource: { rootId: "root-a", relativePath: "src/main.ts" },
            lineNumber: 9,
            columnNumber: 3,
            preview: "const needle = true;",
          },
        ],
        roots: [
          {
            rootId: "root-a",
            rootName: "Root",
            state: "complete",
            filesScanned: 1,
            bytesScanned: 20,
            entriesVisited: 1,
            skippedFiles: 0,
            errorCount: 0,
            message: null,
          },
        ],
        truncated: false,
        limitReasons: [],
        elapsedMs: 1,
      });
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "search",
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "Root",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      nativeTransitionEpoch: 7,
      openFile,
    });

    render(<RightSidebar compact={false} />);
    const query = screen.getByLabelText("Search files");
    const form = query.closest("form");
    expect(form).not.toBeNull();
    fireEvent.change(query, {
      target: { value: "needle" },
    });
    fireEvent.click(within(form!).getByRole("button", { name: "Search" }));

    const result = await screen.findByRole("button", {
      name: "Open src/main.ts at line 9, column 3",
    });
    expect(search).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        expectedTransitionEpoch: 7,
        query: "needle",
        caseSensitive: false,
      },
      expect.any(AbortSignal),
    );

    fireEvent.click(result);
    expect(openFile).toHaveBeenCalledWith({
      rootId: "root-a",
      relativePath: "src/main.ts",
    });
  });
});
