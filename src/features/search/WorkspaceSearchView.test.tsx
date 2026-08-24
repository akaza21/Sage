import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { WorkspaceSearchGateway } from "./workspaceSearchDomain";
import { WorkspaceSearchView } from "./WorkspaceSearchView";

afterEach(cleanup);

describe("Workspace Search view", () => {
  it("searches the active Project and opens a canonical result", async () => {
    const gateway: WorkspaceSearchGateway = {
      search: vi.fn().mockResolvedValue({
        requestId: "request-a",
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        query: "needle",
        matches: [
          {
            resource: { rootId: "root-b", relativePath: "src/main.ts" },
            lineNumber: 9,
            columnNumber: 3,
            preview: "const needle = true;",
          },
        ],
        roots: [
          {
            rootId: "root-a",
            rootName: "Missing",
            state: "unavailable",
            filesScanned: 0,
            bytesScanned: 0,
            entriesVisited: 0,
            skippedFiles: 0,
            errorCount: 1,
            message: "filesystem root is missing",
          },
          {
            rootId: "root-b",
            rootName: "Sage",
            state: "partial",
            filesScanned: 12,
            bytesScanned: 240,
            entriesVisited: 15,
            skippedFiles: 1,
            errorCount: 0,
            message: null,
          },
        ],
        truncated: true,
        continuationToken: null,
        limitReasons: ["fileSizeLimit"],
        elapsedMs: 7,
      }),
    };
    const open = vi.fn();
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={gateway}
        onOpenMatch={open}
      />,
    );

    fireEvent.change(screen.getByLabelText("Search files"), {
      target: { value: "needle" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    const result = await screen.findByRole("button", {
      name: "Open src/main.ts at line 9, column 3",
    });
    expect(
      screen.getByText("Some files exceeded the per-file size limit."),
    ).toBeVisible();
    expect(
      screen.getByText("Missing: filesystem root is missing"),
    ).toBeVisible();
    result.focus();
    fireEvent.keyDown(result, { key: "Enter" });
    expect(result).toHaveFocus();
    expect(open).toHaveBeenCalledWith({
      projectId: "project-a",
      transitionEpoch: 4,
      resource: { rootId: "root-b", relativePath: "src/main.ts" },
      lineNumber: 9,
      columnNumber: 3,
      query: "needle",
      caseSensitive: false,
    });
    await waitFor(() => expect(gateway.search).toHaveBeenCalledTimes(1));
  });

  it("keeps search unavailable when no Project is open", () => {
    render(
      <WorkspaceSearchView
        projectId={null}
        transitionEpoch={0}
        indexGeneration={0}
        gateway={{ search: vi.fn() }}
        onOpenMatch={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "Search" })).toBeDisabled();
    expect(
      screen.getByText("Open a Project to search its files."),
    ).toBeVisible();
  });

  it("shows Continue search while a token remains under the page cap", async () => {
    const token =
      '{"v":1,"g":0,"e":4,"c":0,"q":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","r":"root-a","p":"safe.txt","l":1,"o":1}';
    const gateway: WorkspaceSearchGateway = {
      search: vi.fn().mockResolvedValue({
        requestId: "request-a",
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        query: "needle",
        matches: [
          {
            resource: { rootId: "root-a", relativePath: "safe.txt" },
            lineNumber: 1,
            columnNumber: 1,
            preview: "needle",
          },
        ],
        roots: [
          {
            rootId: "root-a",
            rootName: "Sage",
            state: "partial",
            filesScanned: 1,
            bytesScanned: 6,
            entriesVisited: 1,
            skippedFiles: 1,
            errorCount: 0,
            message: null,
          },
        ],
        truncated: true,
        continuationToken: token,
        limitReasons: ["fileSizeLimit"],
        elapsedMs: 4,
      }),
    };
    render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={0}
        gateway={gateway}
        onOpenMatch={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Search files"), {
      target: { value: "needle" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(
      await screen.findByRole("button", { name: "Continue search" }),
    ).toBeVisible();
  });
});
