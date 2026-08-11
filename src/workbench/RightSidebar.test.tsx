import {
  act,
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
    const openSearchMatch = vi.fn(async () => undefined);
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
      openSearchMatch,
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
    expect(openSearchMatch).toHaveBeenCalledWith({
      projectId: "project-a",
      transitionEpoch: 7,
      resource: { rootId: "root-a", relativePath: "src/main.ts" },
      lineNumber: 9,
      columnNumber: 3,
      query: "needle",
      caseSensitive: false,
    });
  });
});

describe("RightSidebar Source Control mutations", () => {
  it("stages and unstages exact root-qualified changes with accessible actions", () => {
    const mutateRepositoryFile = vi.fn(async () => true);
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "sourceControl",
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "Alpha",
            ordinal: 0,
            availability: "available",
            message: null,
          },
          {
            id: "root-b",
            name: "Beta",
            ordinal: 1,
            availability: "available",
            message: null,
          },
        ],
      },
      repositories: {
        roots: [
          {
            rootId: "root-a",
            state: "repository",
            branch: "main",
            changes: [
              {
                resource: { rootId: "root-a", relativePath: "same.txt" },
                indexStatus: "unchanged",
                worktreeStatus: "modified",
                additions: 1,
                deletions: 0,
              },
            ],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
          {
            rootId: "root-b",
            state: "repository",
            branch: "main",
            changes: [
              {
                resource: { rootId: "root-b", relativePath: "same.txt" },
                indexStatus: "modified",
                worktreeStatus: "modified",
                additions: 2,
                deletions: 1,
              },
            ],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 2,
      },
      mutateRepositoryFile,
    });

    render(<RightSidebar compact={false} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Stage same.txt in Alpha" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Unstage same.txt in Beta" }),
    );

    expect(mutateRepositoryFile).toHaveBeenNthCalledWith(
      1,
      { rootId: "root-a", relativePath: "same.txt" },
      "stage",
    );
    expect(mutateRepositoryFile).toHaveBeenNthCalledWith(
      2,
      { rootId: "root-b", relativePath: "same.txt" },
      "unstage",
    );
  });

  it("disables mutation controls during mutations and Project transitions", () => {
    const resource = { rootId: "root-a", relativePath: "file.txt" };
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "sourceControl",
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "Alpha",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      repositories: {
        roots: [
          {
            rootId: "root-a",
            state: "repository",
            branch: "main",
            changes: [
              {
                resource,
                indexStatus: "unchanged",
                worktreeStatus: "modified",
                additions: 1,
                deletions: 0,
              },
            ],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 1,
      },
      repositoryMutation: { resource, action: "stage" },
    });

    render(<RightSidebar compact={false} />);
    const stage = screen.getByRole("button", {
      name: "Stage file.txt in Alpha",
    });
    const refresh = screen.getByRole("button", {
      name: "Refresh Source Control",
    });
    expect(stage).toBeDisabled();
    expect(refresh).toBeDisabled();

    act(() => {
      useWorkbenchStore.setState({
        repositoryMutation: null,
        projectTransitioning: true,
      });
    });
    expect(stage).toBeDisabled();
    expect(refresh).toBeDisabled();
  });

  it("does not offer stage or unstage for conflicted files", () => {
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "sourceControl",
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "Alpha",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      repositories: {
        roots: [
          {
            rootId: "root-a",
            state: "repository",
            branch: "main",
            changes: [
              {
                resource: { rootId: "root-a", relativePath: "conflict.txt" },
                indexStatus: "conflicted",
                worktreeStatus: "conflicted",
                additions: null,
                deletions: null,
              },
            ],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 1,
      },
    });

    render(<RightSidebar compact={false} />);
    expect(
      screen.queryByRole("button", { name: /^(Stage|Unstage) / }),
    ).toBeNull();
  });
});

describe("RightSidebar Explorer New File", () => {
  it("announces a committed creation warning outside the closed dialog", () => {
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "explorer",
      project: { id: "project-a", name: "A", roots: [] },
      fileCreateNotice:
        "File created, but its durability could not be confirmed.",
    });

    render(<RightSidebar compact={false} />);

    expect(screen.getByRole("status")).toHaveTextContent(
      "File created, but its durability could not be confirmed.",
    );
  });

  it("previews an active file's parent, reports errors, and restores trigger focus", async () => {
    const createProjectFile = vi.fn(async () => {
      useWorkbenchStore.setState({ fileCreateError: "File already exists." });
      return false;
    });
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "explorer",
      phase: "ready",
      projectGeneration: 2,
      nativeTransitionEpoch: 3,
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "Root A",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      selectedRootId: "root-a",
      selectedResource: { rootId: "root-a", relativePath: "src/main.ts" },
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
      activeTabId: "file-a",
      directoryListings: {},
      createProjectFile,
      fileCreateError: null,
      fileCreateInFlight: false,
    });

    render(<RightSidebar compact={false} />);
    const trigger = screen.getByRole("button", { name: "New File" });
    fireEvent.click(trigger);
    const input = await screen.findByLabelText("File name");
    expect(input).toHaveFocus();
    fireEvent.change(input, { target: { value: "new.ts" } });
    expect(screen.getByText("Path: Root A/src/new.ts")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(createProjectFile).toHaveBeenCalledWith(
      { rootId: "root-a", relativePath: "src" },
      "new.ts",
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "File already exists.",
    );
    useWorkbenchStore.setState({ fileCreateInFlight: true });
    await vi.waitFor(() => expect(input).toBeDisabled());
    expect(screen.getByRole("button", { name: "Creating…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    useWorkbenchStore.setState({ fileCreateInFlight: false });
    const cancel = screen.getByRole("button", { name: "Cancel" });
    await vi.waitFor(() => expect(cancel).toBeEnabled());
    fireEvent.click(cancel);
    await vi.waitFor(() => expect(trigger).toHaveFocus());
  });
});
