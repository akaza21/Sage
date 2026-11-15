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
import { resourceKey } from "../domain/resourceIdentity";
import type { ResourceRef, RootRepositoryStatus } from "../domain/workbench";
import { workspaceSearchDesktop } from "../features/search/workspaceSearchDesktop";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { RightSidebar } from "./RightSidebar";

const projectPort = getWorkbenchPorts().project;
const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
  Reflect.deleteProperty(Element.prototype, "scrollIntoView");
});

function openExplorerActions() {
  fireEvent.click(
    screen.getByRole("button", { name: "More Explorer actions" }),
  );
  return screen.getByRole("menu", { name: "Explorer actions" });
}

describe("RightSidebar beta tool rail", () => {
  it("exposes only backed Project tools", () => {
    useWorkbenchStore.setState({ sidebarOpen: false });

    render(<RightSidebar compact={false} />);

    expect(
      screen.getByRole("button", { name: "Explorer" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Source Control" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Preview" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Problems" })).toBeNull();
    expect(screen.getByRole("button", { name: "Outline" })).toBeInTheDocument();
  });
});

describe("RightSidebar Search", () => {
  it("searches the active Project and opens a result through the workbench", async () => {
    const openFileLocation = vi.fn(async () => undefined);
    const search = vi
      .spyOn(workspaceSearchDesktop, "search")
      .mockResolvedValue({
        requestId: "request-a",
        projectId: "project-a",
        transitionEpoch: 7,
        indexGeneration: 0,
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
        continuationToken: null,
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
      openFileLocation,
    });

    render(<RightSidebar compact={false} />);
    const query = await screen.findByLabelText("Search text");
    const form = query.closest("form");
    expect(form).not.toBeNull();
    fireEvent.change(query, {
      target: { value: "needle" },
    });
    fireEvent.submit(form!);

    const result = await screen.findByRole("button", {
      name: "Open src/main.ts at line 9, column 3",
    });
    expect(search).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        expectedTransitionEpoch: 7,
        indexGeneration: 0,
        continuationToken: null,
        query: "needle",
        caseSensitive: false,
        wholeWord: false,
        includePatterns: [],
        excludePatterns: [],
      },
      expect.any(AbortSignal),
    );

    fireEvent.click(result);
    expect(openFileLocation).toHaveBeenCalledWith({
      projectId: "project-a",
      transitionEpoch: 7,
      resource: { rootId: "root-a", relativePath: "src/main.ts" },
      lineNumber: 9,
      columnNumber: 3,
      selection: {
        kind: "match",
        query: "needle",
        caseSensitive: false,
      },
    });
  });
});

describe("RightSidebar Source Control mutations", () => {
  it("commits staged changes per root and keeps root-scoped failures in Source Control", async () => {
    const commitRepository = vi
      .fn<(rootId: string, message: string) => Promise<boolean>>()
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(false);
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
                resource: { rootId: "root-a", relativePath: "staged.txt" },
                indexStatus: "modified",
                worktreeStatus: "unchanged",
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
            changes: [],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 2,
      },
      commitRepository,
    });

    render(<RightSidebar compact={false} />);
    const alphaMessage = await screen.findByRole("textbox", {
      name: "Commit message for Alpha",
    });
    const alphaCommit = screen.getByRole("button", {
      name: "Commit staged changes in Alpha",
    });
    expect(alphaCommit).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "Commit staged changes in Beta",
      }),
    ).toBeDisabled();

    fireEvent.change(alphaMessage, { target: { value: "é".repeat(4_096) } });
    expect(alphaCommit).toBeEnabled();
    fireEvent.change(alphaMessage, { target: { value: "é".repeat(4_097) } });
    expect(alphaCommit).toBeDisabled();
    expect(alphaMessage).toHaveAttribute("aria-invalid", "true");
    expect(
      screen.getByRole("alert", { name: "Alpha commit message invalid" }),
    ).toHaveTextContent("8192 UTF-8 bytes or fewer");

    fireEvent.change(alphaMessage, { target: { value: "  Root A only  " } });
    fireEvent.keyDown(alphaMessage, { key: "Enter", ctrlKey: true });
    await vi.waitFor(() =>
      expect(commitRepository).toHaveBeenCalledWith(
        "root-a",
        "  Root A only  ",
      ),
    );
    await vi.waitFor(() => expect(alphaMessage).toHaveValue(""));
    await vi.waitFor(() => expect(alphaMessage).toHaveFocus());

    fireEvent.change(alphaMessage, { target: { value: "Retry me" } });
    fireEvent.submit(alphaMessage.closest("form")!);
    await vi.waitFor(() => expect(commitRepository).toHaveBeenCalledTimes(2));
    act(() => {
      useWorkbenchStore.setState({
        repositoryCommitError: {
          rootId: "root-a",
          kind: "failed",
          message: "Local Git identity is missing",
        },
      });
    });
    expect(
      screen.getByRole("alert", { name: "Alpha commit failed" }),
    ).toHaveTextContent("Local Git identity is missing");
    expect(alphaMessage).toHaveValue("Retry me");
    expect(alphaMessage).toHaveFocus();
    expect(useWorkbenchStore.getState().error).toBeNull();
  });

  it("disambiguates duplicate root names in every commit control", () => {
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "sourceControl",
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "Shared",
            ordinal: 0,
            availability: "available",
            message: null,
          },
          {
            id: "root-b",
            name: "Shared",
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
            changes: [],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
          {
            rootId: "root-b",
            state: "repository",
            branch: "main",
            changes: [],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 2,
      },
    });

    render(<RightSidebar compact={false} />);

    expect(
      screen.getByRole("textbox", {
        name: "Commit message for Shared (root 1)",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("textbox", {
        name: "Commit message for Shared (root 2)",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("Shared (root 1)")).toBeInTheDocument();
    expect(screen.getByText("Shared (root 2)")).toBeInTheDocument();
  });

  it("keeps the retained draft focusable after an effect-unknown projection", async () => {
    const root: RootRepositoryStatus = {
      rootId: "root-a",
      state: "repository",
      branch: "main",
      changes: [
        {
          resource: { rootId: "root-a", relativePath: "staged.txt" },
          indexStatus: "modified",
          worktreeStatus: "unchanged",
          additions: 1,
          deletions: 0,
        },
      ],
      truncated: false,
      elapsedMs: 1,
      message: null,
    };
    const commitRepository = vi.fn(async () => {
      useWorkbenchStore.setState({
        repositories: {
          roots: [
            {
              ...root,
              changes: [],
              truncated: true,
              message:
                "Commit outcome is uncertain. Refresh Source Control to reconcile.",
            },
          ],
          truncated: true,
          elapsedMs: 2,
        },
        repositoryCommitError: {
          rootId: "root-a",
          kind: "effectUnknown",
          message: "Commit outcome is uncertain",
        },
      });
      return false;
    });
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
      repositories: { roots: [root], truncated: false, elapsedMs: 1 },
      commitRepository,
    });

    render(<RightSidebar compact={false} />);
    const message = screen.getByRole("textbox", {
      name: "Commit message for Alpha",
    });
    fireEvent.change(message, { target: { value: "Retain this draft" } });
    fireEvent.submit(message.closest("form")!);

    await vi.waitFor(() => expect(commitRepository).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(message).toHaveFocus());
    expect(message).toBeEnabled();
    expect(message).toHaveValue("Retain this draft");
    fireEvent.change(message, { target: { value: "Edit retained draft" } });
    expect(message).toHaveValue("Edit retained draft");
    expect(
      screen.getByRole("alert", { name: "Alpha commit failed" }),
    ).toHaveTextContent("Commit outcome is uncertain");
    expect(
      screen.getByRole("button", { name: "Commit staged changes in Alpha" }),
    ).toBeDisabled();
  });

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

  it("requires an accessible inline confirmation before discarding one safe worktree change", async () => {
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
                resource: { rootId: "root-a", relativePath: "partial.txt" },
                originalResource: null,
                indexStatus: "modified",
                worktreeStatus: "modified",
                additions: 1,
                deletions: 1,
              },
              {
                resource: { rootId: "root-a", relativePath: "deleted.txt" },
                indexStatus: "unchanged",
                worktreeStatus: "deleted",
                additions: 0,
                deletions: 2,
              },
              {
                resource: { rootId: "root-a", relativePath: "type.txt" },
                indexStatus: "unchanged",
                worktreeStatus: "typeChanged",
                additions: null,
                deletions: null,
              },
              {
                resource: { rootId: "root-a", relativePath: "untracked.txt" },
                indexStatus: "unchanged",
                worktreeStatus: "untracked",
                additions: null,
                deletions: null,
              },
              {
                resource: { rootId: "root-a", relativePath: "renamed.txt" },
                originalResource: {
                  rootId: "root-a",
                  relativePath: "before.txt",
                },
                indexStatus: "renamed",
                worktreeStatus: "modified",
                additions: 0,
                deletions: 0,
              },
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
      mutateRepositoryFile,
    });

    render(<RightSidebar compact={false} />);
    const discardButton = () =>
      screen.getByRole("button", {
        name: "Discard unstaged changes to partial.txt in Alpha",
      });
    const discard = discardButton();
    expect(
      screen.getByRole("button", {
        name: "Discard unstaged changes to deleted.txt in Alpha",
      }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", {
        name: "Discard unstaged changes to type.txt in Alpha",
      }),
    ).toBeEnabled();
    expect(
      screen.queryByRole("button", {
        name: /Discard unstaged changes to untracked/,
      }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: /Discard unstaged changes to renamed/,
      }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: /Discard unstaged changes to conflict/,
      }),
    ).toBeNull();

    fireEvent.click(discard);
    expect(mutateRepositoryFile).not.toHaveBeenCalled();
    expect(
      screen.getByText(
        "Discard unstaged changes to partial.txt in Alpha? This cannot be undone.",
      ),
    ).toBeInTheDocument();
    const confirm = screen.getByRole("button", {
      name: "Confirm discard of partial.txt in Alpha",
    });
    const cancel = screen.getByRole("button", {
      name: "Cancel discard of partial.txt in Alpha",
    });
    expect(cancel).toHaveFocus();
    expect(confirm).toHaveAccessibleDescription(
      "Discard unstaged changes to partial.txt in Alpha? This cannot be undone.",
    );
    fireEvent.click(cancel);
    expect(screen.queryByText(/This cannot be undone/)).toBeNull();
    await vi.waitFor(() => expect(discardButton()).toHaveFocus());

    fireEvent.click(discardButton());
    fireEvent.keyDown(
      screen.getByRole("group", {
        name: "Confirm discard of partial.txt in Alpha",
      }),
      { key: "Escape" },
    );
    expect(screen.queryByText(/This cannot be undone/)).toBeNull();
    await vi.waitFor(() => expect(discardButton()).toHaveFocus());

    fireEvent.click(
      screen.getByRole("button", {
        name: "Discard unstaged changes to partial.txt in Alpha",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "Confirm discard of partial.txt in Alpha",
      }),
    );
    expect(mutateRepositoryFile).toHaveBeenCalledWith(
      { rootId: "root-a", relativePath: "partial.txt" },
      "discard",
      true,
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

  it("does not offer stage, unstage, or discard for conflicted files", () => {
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
      screen.queryByRole("button", { name: /^(Stage|Unstage|Discard) / }),
    ).toBeNull();
    expect(screen.getByText("Conflicts")).toBeInTheDocument();
    expect(screen.getByText("Conflict")).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Keep ours for conflict.txt in Alpha",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Keep theirs for conflict.txt in Alpha",
      }),
    ).toBeInTheDocument();
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
    expect(screen.getByRole("dialog", { name: "New File" })).toHaveClass(
      "new-file-dialog--editor",
    );
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

describe("RightSidebar Explorer New Folder", () => {
  it("announces a committed folder warning outside the closed dialog", () => {
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "explorer",
      project: { id: "project-a", name: "A", roots: [] },
      folderCreateNotice:
        "Folder created, but its durability could not be confirmed.",
    });

    render(<RightSidebar compact={false} />);

    expect(screen.getByRole("status")).toHaveTextContent(
      "Folder created, but its durability could not be confirmed.",
    );
  });

  it("previews the exact root-qualified parent, reports errors, and restores focus", async () => {
    const createProjectFolder = vi.fn(async () => {
      useWorkbenchStore.setState({
        folderCreateError: "Folder already exists.",
      });
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
            id: "root-b",
            name: "Root B",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      selectedRootId: "root-b",
      selectedResource: { rootId: "root-b", relativePath: "src" },
      tabs: [],
      activeTabId: null,
      directoryListings: {
        [resourceKey({ rootId: "root-b", relativePath: "src" })]: {
          phase: "ready",
          value: {
            resource: { rootId: "root-b", relativePath: "src" },
            entries: [],
            truncated: false,
          },
        },
      },
      createProjectFolder,
      folderCreateError: null,
      folderCreateInFlight: false,
    });

    render(<RightSidebar compact={false} />);
    const trigger = screen.getByRole("button", { name: "New Folder" });
    fireEvent.click(trigger);
    const input = await screen.findByLabelText("Folder name");
    expect(input).toHaveFocus();
    expect(screen.getByRole("dialog", { name: "New Folder" })).toHaveClass(
      "new-file-dialog--editor",
    );
    fireEvent.change(input, { target: { value: "nested" } });
    expect(screen.getByText("Path: Root B/src/nested")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create folder" }));
    expect(createProjectFolder).toHaveBeenCalledWith(
      { rootId: "root-b", relativePath: "src" },
      "nested",
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Folder already exists.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await vi.waitFor(() => expect(trigger).toHaveFocus());
  });
});

describe("RightSidebar Explorer Rename File", () => {
  function renameState(
    renameProjectFile: (
      source: ResourceRef,
      basename: string,
    ) => Promise<boolean>,
  ) {
    const source = { rootId: "root-a", relativePath: "src/old.ts" };
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
      selectedResource: source,
      tabs: [],
      activeTabId: null,
      directoryListings: {
        [resourceKey({ rootId: "root-a", relativePath: "src" })]: {
          phase: "ready",
          value: {
            resource: { rootId: "root-a", relativePath: "src" },
            entries: [
              { name: "old.ts", resource: source, kind: "file", byteLen: 9 },
            ],
            truncated: false,
          },
        },
      },
      renameProjectFile,
      fileRenameError: null,
      fileRenameInFlight: false,
      projectTransitioning: false,
    });
    return source;
  }

  it("retains native errors, previews the target, then closes and restores focus", async () => {
    const renameProjectFile = vi
      .fn()
      .mockImplementationOnce(async () => {
        useWorkbenchStore.setState({
          fileRenameError: "Target already exists.",
        });
        return false;
      })
      .mockResolvedValueOnce(true);
    const source = renameState(renameProjectFile);

    render(<RightSidebar compact={false} />);
    const trigger = within(openExplorerActions()).getByRole("menuitem", {
      name: "Rename File",
    });
    fireEvent.click(trigger);
    const input = await screen.findByLabelText("File name");
    expect(input).toHaveFocus();
    fireEvent.change(input, { target: { value: "new.ts" } });
    expect(screen.getByText("Path: Root A/src/new.ts")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    expect(renameProjectFile).toHaveBeenLastCalledWith(source, "new.ts");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Target already exists.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("button", { name: "More Explorer actions" }),
    ).toHaveFocus();
  });

  it("requires the source file and diff tabs to be closed", () => {
    const renameProjectFile = vi.fn(async () => true);
    const source = renameState(renameProjectFile);
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "file-a",
          kind: "file",
          title: "old.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: source,
        },
      ],
    });

    render(<RightSidebar compact={false} />);
    const menu = openExplorerActions();
    expect(
      within(menu).queryByRole("menuitem", { name: "Rename File" }),
    ).not.toBeInTheDocument();
    expect(
      within(menu).getByText(
        "Close this resource before renaming or recycling it.",
      ),
    ).toBeInTheDocument();
  });

  it("closes instead of retargeting when another file is selected in the same Project", async () => {
    const renameProjectFile = vi.fn(async () => true);
    const source = renameState(renameProjectFile);
    const replacement = { rootId: "root-a", relativePath: "src/other.ts" };

    render(<RightSidebar compact={false} />);
    fireEvent.click(
      within(openExplorerActions()).getByRole("menuitem", {
        name: "Rename File",
      }),
    );
    expect(await screen.findByLabelText("File name")).toBeInTheDocument();

    act(() => {
      useWorkbenchStore.setState((state) => ({
        selectedResource: replacement,
        directoryListings: {
          ...state.directoryListings,
          [resourceKey({ rootId: "root-a", relativePath: "src" })]: {
            phase: "ready",
            value: {
              resource: { rootId: "root-a", relativePath: "src" },
              entries: [
                { name: "old.ts", resource: source, kind: "file", byteLen: 9 },
                {
                  name: "other.ts",
                  resource: replacement,
                  kind: "file",
                  byteLen: 9,
                },
              ],
              truncated: false,
            },
          },
        },
      }));
    });

    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(renameProjectFile).not.toHaveBeenCalled();
  });

  it("keeps its own transition mounted and exposes a native failure", async () => {
    let rejectRename!: (error: Error) => void;
    const pendingRename = new Promise<never>((_resolve, reject) => {
      rejectRename = reject;
    });
    vi.spyOn(projectPort, "persistProjectLayout").mockResolvedValue();
    vi.spyOn(projectPort, "renameProjectFile").mockReturnValue(pendingRename);
    renameState(initialState.renameProjectFile);

    render(<RightSidebar compact={false} />);
    fireEvent.click(
      within(openExplorerActions()).getByRole("menuitem", {
        name: "Rename File",
      }),
    );
    fireEvent.change(await screen.findByLabelText("File name"), {
      target: { value: "new.ts" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));

    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState()).toMatchObject({
        projectTransitioning: true,
        fileRenameInFlight: true,
      }),
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Renaming/ })).toBeDisabled();

    await vi.waitFor(() =>
      expect(projectPort.renameProjectFile).toHaveBeenCalled(),
    );
    act(() => rejectRename(new Error("Native rename failed safely.")));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Native rename failed safely.",
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});

describe("RightSidebar Explorer folder lifecycle", () => {
  it("renames a selected empty folder through its dedicated control", async () => {
    const source = { rootId: "root-a", relativePath: "src/empty" };
    const parent = { rootId: "root-a", relativePath: "src" };
    const renameProjectFolder = vi.fn(async () => true);
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "explorer",
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
      selectedResource: source,
      directoryListings: {
        [resourceKey(parent)]: {
          phase: "ready",
          value: {
            resource: parent,
            truncated: false,
            entries: [
              {
                name: "empty",
                resource: source,
                kind: "directory",
                byteLen: null,
              },
            ],
          },
        },
        [resourceKey(source)]: {
          phase: "ready",
          value: { resource: source, truncated: false, entries: [] },
        },
      },
      renameProjectFolder,
      fileRenameError: null,
      fileRenameInFlight: false,
      projectTransitioning: false,
    });

    render(<RightSidebar compact={false} />);
    fireEvent.click(
      within(openExplorerActions()).getByRole("menuitem", {
        name: "Rename Folder",
      }),
    );
    fireEvent.change(await screen.findByLabelText("Folder name"), {
      target: { value: "renamed" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));

    expect(renameProjectFolder).toHaveBeenCalledWith(source, "renamed");
  });

  it("closes instead of retargeting when another empty folder is selected", async () => {
    const source = { rootId: "root-a", relativePath: "src/empty" };
    const replacement = { rootId: "root-a", relativePath: "src/other" };
    const parent = { rootId: "root-a", relativePath: "src" };
    const renameProjectFolder = vi.fn(async () => true);
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "explorer",
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
      selectedResource: source,
      tabs: [],
      activeEditor: null,
      directoryListings: {
        [resourceKey(parent)]: {
          phase: "ready",
          value: {
            resource: parent,
            truncated: false,
            entries: [
              {
                name: "empty",
                resource: source,
                kind: "directory",
                byteLen: null,
              },
              {
                name: "other",
                resource: replacement,
                kind: "directory",
                byteLen: null,
              },
            ],
          },
        },
        [resourceKey(source)]: {
          phase: "ready",
          value: { resource: source, truncated: false, entries: [] },
        },
        [resourceKey(replacement)]: {
          phase: "ready",
          value: { resource: replacement, truncated: false, entries: [] },
        },
      },
      renameProjectFolder,
      fileRenameError: null,
      fileRenameInFlight: false,
      projectTransitioning: false,
    });

    render(<RightSidebar compact={false} />);
    fireEvent.click(
      within(openExplorerActions()).getByRole("menuitem", {
        name: "Rename Folder",
      }),
    );
    expect(await screen.findByLabelText("Folder name")).toBeInTheDocument();

    act(() => useWorkbenchStore.setState({ selectedResource: replacement }));

    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(renameProjectFolder).not.toHaveBeenCalled();
  });

  it("refreshes the expanded truncated folder instead of an earlier collapsed cache", () => {
    const root = { rootId: "root-a", relativePath: "" };
    const collapsed = { rootId: "root-a", relativePath: "a-collapsed" };
    const expanded = { rootId: "root-a", relativePath: "b-expanded" };
    const loadDirectory = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "explorer",
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
      selectedResource: root,
      directoryListings: {
        [resourceKey(root)]: {
          phase: "ready",
          value: {
            resource: root,
            truncated: false,
            entries: [
              {
                name: "a-collapsed",
                resource: collapsed,
                kind: "directory",
                byteLen: null,
              },
              {
                name: "b-expanded",
                resource: expanded,
                kind: "directory",
                byteLen: null,
              },
            ],
          },
        },
        [resourceKey(collapsed)]: {
          phase: "ready",
          value: { resource: collapsed, truncated: true, entries: [] },
        },
        [resourceKey(expanded)]: {
          phase: "ready",
          value: { resource: expanded, truncated: true, entries: [] },
        },
      },
      expandedDirectories: { [resourceKey(expanded)]: true },
      loadDirectory,
    });

    render(<RightSidebar compact={false} />);
    expect(
      screen.getByText("More entries are hidden by the view limit."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh folder" }));
    expect(loadDirectory).toHaveBeenCalledWith(expanded, true);
  });
});

describe("RightSidebar Explorer recoverable trash", () => {
  function trashState(
    selectedResource: ResourceRef,
    entryKind: "file" | "directory",
    trashProjectResource: (
      source: ResourceRef,
      kind: "file" | "emptyFolder",
    ) => Promise<boolean>,
  ) {
    const parent = {
      rootId: selectedResource.rootId,
      relativePath: "src",
    };
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
      selectedRootId: selectedResource.rootId,
      selectedResource,
      tabs: [],
      activeTabId: null,
      activeEditor: null,
      directoryListings: {
        [resourceKey(parent)]: {
          phase: "ready",
          value: {
            resource: parent,
            entries: [
              {
                name: selectedResource.relativePath.split("/").at(-1)!,
                resource: selectedResource,
                kind: entryKind,
                byteLen: entryKind === "file" ? 9 : null,
              },
            ],
            truncated: false,
          },
        },
        ...(entryKind === "directory"
          ? {
              [resourceKey(selectedResource)]: {
                phase: "ready" as const,
                value: {
                  resource: selectedResource,
                  entries: [],
                  truncated: false,
                },
              },
            }
          : {}),
      },
      trashProjectResource,
      resourceTrashError: null,
      resourceTrashInFlight: false,
      projectTransitioning: false,
    });
  }

  it("confirms an exact multi-root file, retains native errors, and restores focus", async () => {
    const source = { rootId: "root-b", relativePath: "src/old.ts" };
    const trashProjectResource = vi
      .fn()
      .mockImplementationOnce(async () => {
        useWorkbenchStore.setState({
          resourceTrashError: "Trash operation is unavailable.",
        });
        return false;
      })
      .mockResolvedValueOnce(true);
    trashState(source, "file", trashProjectResource);

    render(<RightSidebar compact={false} />);
    const trigger = within(openExplorerActions()).getByRole("menuitem", {
      name: "Move to Trash",
    });
    fireEvent.click(trigger);
    expect(
      await screen.findByRole("heading", { name: "Move to Trash?" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Path: Beta/src/old.ts")).toBeInTheDocument();
    expect(screen.getByText(/Move this file/)).toBeInTheDocument();

    const dialog = screen.getByRole("dialog");
    act(() => {
      useWorkbenchStore.setState({
        resourceTrashInFlight: true,
      });
    });
    expect(
      within(dialog).getByRole("button", { name: "Moving…" }),
    ).toBeDisabled();
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toBeDisabled();
    act(() => {
      useWorkbenchStore.setState({
        resourceTrashInFlight: false,
      });
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Move to Trash" }),
    );
    expect(trashProjectResource).toHaveBeenLastCalledWith(source, "file");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Trash operation is unavailable.",
    );

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Move to Trash" }),
    );
    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("button", { name: "More Explorer actions" }),
    ).toHaveFocus();
  });

  it("allows only a proven-empty folder and blocks an open descendant Diff", async () => {
    const source = { rootId: "root-b", relativePath: "src/empty" };
    const trashProjectResource = vi.fn(async () => true);
    trashState(source, "directory", trashProjectResource);

    render(<RightSidebar compact={false} />);
    const trigger = within(openExplorerActions()).getByRole("menuitem", {
      name: "Move to Trash",
    });
    fireEvent.click(trigger);
    expect(
      await screen.findByText(/Move this empty folder/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    act(() => {
      useWorkbenchStore.setState({
        tabs: [
          {
            id: "diff-child",
            kind: "diff",
            title: "child.ts",
            position: 0,
            pinned: false,
            dirty: false,
            resource: {
              rootId: "root-b",
              relativePath: "src/empty/child.ts",
            },
          },
        ],
      });
    });
    const menu = openExplorerActions();
    expect(
      within(menu).queryByRole("menuitem", { name: "Move to Trash" }),
    ).not.toBeInTheDocument();
    expect(
      within(menu).getByText(
        "Close this resource before renaming or recycling it.",
      ),
    ).toBeInTheDocument();
  });

  it("closes instead of retargeting when another file is selected before recycle", async () => {
    const source = { rootId: "root-b", relativePath: "src/old.ts" };
    const replacement = { rootId: "root-b", relativePath: "src/other.ts" };
    const trashProjectResource = vi.fn(async () => true);
    trashState(source, "file", trashProjectResource);
    useWorkbenchStore.setState((state) => ({
      directoryListings: {
        ...state.directoryListings,
        [resourceKey({ rootId: "root-b", relativePath: "src" })]: {
          phase: "ready",
          value: {
            resource: { rootId: "root-b", relativePath: "src" },
            entries: [
              { name: "old.ts", resource: source, kind: "file", byteLen: 9 },
              {
                name: "other.ts",
                resource: replacement,
                kind: "file",
                byteLen: 9,
              },
            ],
            truncated: false,
          },
        },
      },
    }));

    render(<RightSidebar compact={false} />);
    fireEvent.click(
      within(openExplorerActions()).getByRole("menuitem", {
        name: "Move to Trash",
      }),
    );
    expect(await screen.findByRole("dialog")).toBeInTheDocument();

    act(() => useWorkbenchStore.setState({ selectedResource: replacement }));

    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(trashProjectResource).not.toHaveBeenCalled();
  });

  it("returns focus to More after the real store transition removes the source", async () => {
    const source = { rootId: "root-b", relativePath: "src/old.ts" };
    const parent = { rootId: "root-b", relativePath: "src" };
    const root = { rootId: "root-b", relativePath: "" };
    trashState(source, "file", initialState.trashProjectResource);
    useWorkbenchStore.setState((state) => ({
      directoryListings: {
        ...state.directoryListings,
        [resourceKey(root)]: {
          phase: "ready",
          value: {
            resource: root,
            entries: [
              {
                name: "src",
                resource: parent,
                kind: "directory",
                byteLen: null,
              },
            ],
            truncated: false,
          },
        },
      },
      expandedDirectories: { [resourceKey(root)]: true },
      loadDirectory: vi.fn(async () => undefined),
      refreshRepository: vi.fn(async () => undefined),
    }));
    vi.spyOn(projectPort, "persistProjectLayout").mockResolvedValue();
    vi.spyOn(projectPort, "trashProjectResource").mockResolvedValue({
      outcome: "effectUnknown",
      resourceConfirmed: true,
      projectId: "project-a",
      projectGeneration: 2,
      transitionEpoch: 4,
      source,
      parent,
      parentListing: { resource: parent, entries: [], truncated: false },
      snapshot: null,
      warning: "The source is absent; Explorer is refreshing.",
    });
    Object.defineProperty(Element.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });

    render(<RightSidebar compact={false} />);
    fireEvent.click(
      within(openExplorerActions()).getByRole("menuitem", {
        name: "Move to Trash",
      }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Move to Trash" }),
    );

    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(useWorkbenchStore.getState().selectedResource).toEqual(parent);
    expect(
      screen.getByRole("button", { name: "More Explorer actions" }),
    ).toHaveFocus();
  });

  it("closes once when an external Project transition invalidates the dialog", async () => {
    const source = { rootId: "root-b", relativePath: "src/old.ts" };
    trashState(
      source,
      "file",
      vi.fn(async () => true),
    );

    render(<RightSidebar compact={false} />);
    fireEvent.click(
      within(openExplorerActions()).getByRole("menuitem", {
        name: "Move to Trash",
      }),
    );
    expect(await screen.findByRole("dialog")).toBeInTheDocument();

    act(() => {
      useWorkbenchStore.setState({ projectTransitioning: true });
    });

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
