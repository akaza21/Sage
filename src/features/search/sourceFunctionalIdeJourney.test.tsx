import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { diffCacheKey } from "../../domain/diffIdentity";
import type { EditorDocument, ResourceRef } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { desktop } from "../../services/desktop";
import { ChatLegacyTranscript } from "../chat/ChatLegacyTranscript";
import { ExplorerSurface } from "../files/ExplorerSurface";
import { FileEditorHeader } from "../files/FileEditorHeader";
import { OutlineSurface } from "../outline/OutlineSurface";
import { WorkspaceSearchView } from "./WorkspaceSearchView";

const target: ResourceRef = { rootId: "root-a", relativePath: "src/main.ts" };
const initialState = useWorkbenchStore.getState();

Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
  configurable: true,
  value: () => undefined,
});

function document(
  content: string,
  revision: number,
  dirty: boolean,
): EditorDocument {
  const version = {
    fingerprint: `v-${revision}`,
    byteLen: content.length,
    modifiedAt: null,
  };
  return {
    resource: target,
    disk: version,
    base: version,
    buffer: {
      fingerprint: `buffer-${revision}`,
      byteLen: content.length,
      revision,
      content,
    },
    lineEnding: "lf",
    dirty,
    conflict: null,
  };
}

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

describe("source-functional IDE journey", () => {
  it("chains Project, Explorer, Search, File, edit/Outline, and Diff through UI and workbench ports", async () => {
    const project = getWorkbenchPorts().project;
    const editor = getWorkbenchPorts().editor;
    const repository = getWorkbenchPorts().repository;
    useWorkbenchStore.setState({
      phase: "ready",
      projectGeneration: 1,
      nativeTransitionEpoch: 4,
      projects: [
        { id: "project-a", name: "Journey", rootCount: 1, lastOpenedAt: 1 },
      ],
      project: {
        id: "project-a",
        name: "Journey",
        roots: [
          {
            id: "root-a",
            name: "Journey",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      tabs: [],
      activeTabId: null,
      selectedRootId: "root-a",
      selectedResource: null,
      directoryListings: {},
      expandedDirectories: {},
      diffs: {},
      requestGenerations: {},
      requestIntentSequence: 0,
      activeEditor: null,
      fileNavigationTarget: null,
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
    });
    vi.spyOn(project, "listDirectory").mockResolvedValue({
      resource: { rootId: "root-a", relativePath: "" },
      entries: [
        { resource: target, kind: "file", name: "main.ts", byteLen: 10 },
      ],
      truncated: false,
    });
    vi.spyOn(editor, "openEditorDocument").mockResolvedValue(
      document("let x = 1\n", 0, false),
    );
    vi.spyOn(editor, "snapshotEditorBuffer").mockResolvedValue({
      revision: 1,
      persistedAt: 1,
      dirty: true,
    });
    vi.spyOn(editor, "saveEditorDocument").mockResolvedValue({
      outcome: "saved",
      document: document("let x = 1;\n", 1, false),
    });
    vi.spyOn(desktop, "getRepositoryStatus").mockResolvedValue({
      roots: [],
      truncated: false,
      elapsedMs: 1,
    });
    vi.spyOn(repository, "getFileDiff").mockResolvedValue({
      resource: target,
      hunks: [],
      binary: false,
      truncated: false,
    });
    const explorer = render(<ExplorerSurface />);
    fireEvent.click(screen.getByRole("button", { name: "Journey" }));
    await screen.findByRole("treeitem", { name: "Open file src/main.ts" });
    fireEvent.click(
      screen.getByRole("treeitem", { name: "Open file src/main.ts" }),
    );
    await waitFor(() =>
      expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
        phase: "ready",
        resource: target,
      }),
    );
    expect(project.listDirectory).toHaveBeenCalledWith({
      rootId: "root-a",
      relativePath: "",
    });
    explorer.unmount();

    const search = render(
      <WorkspaceSearchView
        projectId="project-a"
        transitionEpoch={4}
        indexGeneration={1}
        onOpenMatch={(request) =>
          void useWorkbenchStore.getState().openFileLocation({
            projectId: request.projectId,
            transitionEpoch: request.transitionEpoch,
            resource: request.resource,
            lineNumber: request.lineNumber,
            columnNumber: request.columnNumber,
            selection: {
              kind: "match",
              query: request.query,
              caseSensitive: request.caseSensitive,
            },
          })
        }
        gateway={{
          search: async () => ({
            requestId: "journey-search",
            projectId: "project-a",
            transitionEpoch: 4,
            indexGeneration: 1,
            query: "let x",
            matches: [
              {
                resource: target,
                lineNumber: 1,
                columnNumber: 1,
                preview: "let x = 1",
              },
            ],
            roots: [
              {
                rootId: "root-a",
                rootName: "Journey",
                state: "complete",
                filesScanned: 1,
                bytesScanned: 10,
                entriesVisited: 1,
                skippedFiles: 0,
                errorCount: 0,
                message: null,
              },
            ],
            truncated: false,
            continuationToken: null,
            limitReasons: [],
            elapsedMs: 1,
          }),
        }}
      />,
    );
    const workspaceSearchInput = screen.getByRole("searchbox", {
      name: "Search text",
    });
    fireEvent.change(workspaceSearchInput, {
      target: { value: "let x" },
    });
    fireEvent.submit(workspaceSearchInput.closest("form")!);
    const workspaceResult = await screen.findByRole("button", {
      name: "Open src/main.ts at line 1, column 1",
    });
    fireEvent.click(workspaceResult);
    await waitFor(() =>
      expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
        phase: "ready",
        resource: target,
      }),
    );

    search.unmount();
    const outline = render(<OutlineSurface />);
    expect(
      await screen.findByRole("button", {
        name: "Variable: x. src/main.ts, line 1, column 5",
      }),
    ).toBeInTheDocument();
    outline.unmount();

    useWorkbenchStore.getState().updateEditorContent("let x = 1;\n");
    const repositoryRefresh = vi.spyOn(desktop, "getRepositoryStatus");
    const directoryRefresh = vi.spyOn(project, "listDirectory");
    const header = render(
      <FileEditorHeader
        path="src/main.ts"
        lineEnding="lf"
        diskByteLen={10}
        persisting={false}
        dirty
        saving={false}
        transitioning={false}
      />,
    );
    await act(async () => {
      await useWorkbenchStore.getState().saveActiveEditor();
    });
    await waitFor(() =>
      expect(editor.saveEditorDocument).toHaveBeenCalledWith(target),
    );
    expect(repositoryRefresh).toHaveBeenCalled();
    expect(directoryRefresh).toHaveBeenCalledWith({
      rootId: "root-a",
      relativePath: "src",
    });
    header.unmount();
    useWorkbenchStore.setState({
      turnChanges: [
        {
          resource: target,
          indexStatus: "modified",
          worktreeStatus: "modified",
          additions: 1,
          deletions: 1,
        },
      ],
    });
    render(<ChatLegacyTranscript messages={[]} developmentSimulator />);
    expect(screen.queryByRole("button", { name: "Source Control" })).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Open diff for src/main.ts" }),
    );
    await waitFor(() =>
      expect(
        useWorkbenchStore.getState().diffs[diffCacheKey(undefined, target)],
      ).toMatchObject({ phase: "ready" }),
    );
    expect(useWorkbenchStore.getState().tabs).toContainEqual(
      expect.objectContaining({ kind: "diff", resource: target }),
    );
  });
});
