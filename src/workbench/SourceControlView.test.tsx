import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type { RepositoryRef, RootRepositoryStatus } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { SearchablePicker } from "../ui/SearchablePicker";
import type * as SearchablePickerModule from "../ui/SearchablePicker";
import { SourceControlView } from "./SourceControlView";

const { pickerOptionRender, pickerProps } = vi.hoisted(() => ({
  pickerOptionRender: vi.fn(),
  pickerProps: vi.fn(),
}));

vi.mock("../ui/SearchablePicker", async (importOriginal) => {
  const actual = await importOriginal<typeof SearchablePickerModule>();
  function TrackedSearchablePicker(
    props: ComponentProps<typeof SearchablePicker>,
  ) {
    pickerProps(props);
    return (
      <actual.SearchablePicker {...props} onOptionRender={pickerOptionRender} />
    );
  }
  return { ...actual, SearchablePicker: TrackedSearchablePicker };
});

const initialState = useWorkbenchStore.getState();
const rootRepository: RepositoryRef = { rootId: "root-a", relativePath: "" };
const nestedRepository: RepositoryRef = {
  rootId: "root-a",
  relativePath: "packages/tool",
};
const rootStatus: RootRepositoryStatus = {
  rootId: "root-a",
  state: "repository",
  branch: "main",
  branches: ["main", "topic"],
  branchesTruncated: false,
  recentCommits: [
    {
      sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      subject: "second",
    },
    {
      sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      subject: "first",
    },
  ],
  recentCommitsTruncated: false,
  remotes: ["origin", "upstream"],
  remotesTruncated: false,
  changes: [],
  truncated: false,
  elapsedMs: 1,
  message: null,
};
const nestedStatus: RootRepositoryStatus = {
  ...rootStatus,
  changes: [
    {
      resource: { rootId: "root-a", relativePath: "packages/tool/src/app.ts" },
      indexStatus: "modified",
      worktreeStatus: "modified",
      additions: 1,
      deletions: 0,
    },
  ],
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

it("retries inventory discovery after a project transition and clears stale selection", async () => {
  const repository = getWorkbenchPorts().repository;
  const freshRoot: RepositoryRef = { rootId: "root-b", relativePath: "" };
  const discover = vi
    .spyOn(repository, "discoverProjectRepositories")
    .mockResolvedValue({
      projectId: "project-b",
      transitionEpoch: 1,
      refs: [freshRoot],
      truncated: false,
    });

  useWorkbenchStore.setState({
    phase: "ready",
    project: {
      id: "project-b",
      name: "B",
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
    projectGeneration: 1,
    nativeTransitionEpoch: 1,
    projectTransitioning: true,
    repositoryInventoryPhase: "ready",
    repositoryInventoryProjectScope: "project-a\u00000\u00000",
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryStatusPhase: "ready",
  });

  render(<SourceControlView />);

  await waitFor(() => {
    expect(useWorkbenchStore.getState().selectedRepository).toBeNull();
    expect(useWorkbenchStore.getState().repositoryRefs).toEqual([]);
  });
  expect(discover).not.toHaveBeenCalled();

  useWorkbenchStore.setState({ projectTransitioning: false });

  await waitFor(() =>
    expect(discover).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "project-b",
        projectGeneration: 1,
        expectedTransitionEpoch: 1,
      }),
    ),
  );
  await waitFor(() => {
    expect(useWorkbenchStore.getState().selectedRepository).toEqual(freshRoot);
  });
});

it("uses disambiguated project-root labels for repository options", () => {
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: "root-a",
          name: "Workspace",
          ordinal: 0,
          availability: "available",
          message: null,
        },
        {
          id: "root-b",
          name: "Workspace",
          ordinal: 1,
          availability: "available",
          message: null,
        },
      ],
    },
    repositories: { roots: [], truncated: false, elapsedMs: 1 },
    repositoryInventoryProjectScope: "project-a\u00000\u00000",
    repositoryRefs: [
      { rootId: "root-a", relativePath: "packages/tool" },
      { rootId: "root-b", relativePath: "packages/tool" },
    ],
    selectedRepository: { rootId: "root-a", relativePath: "packages/tool" },
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  fireEvent.click(screen.getByRole("button", { name: "Select repository" }));

  expect(
    screen.getByRole("option", { name: "Workspace (root 1) — packages/tool" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("option", { name: "Workspace (root 2) — packages/tool" }),
  ).toBeInTheDocument();

  const initialPickerProps = pickerProps.mock.lastCall?.[0];
  pickerProps.mockClear();
  pickerOptionRender.mockClear();
  act(() => {
    useWorkbenchStore.setState({
      selectedResource: { rootId: "root-a", relativePath: "unrelated.ts" },
    });
  });
  const rerenderedPickerProps = pickerProps.mock.lastCall?.[0];
  expect(rerenderedPickerProps?.candidates).toBe(
    initialPickerProps?.candidates,
  );
  expect(rerenderedPickerProps?.onSelect).toBe(initialPickerProps?.onSelect);
  expect(pickerOptionRender).not.toHaveBeenCalled();

  const picker = screen.getByRole("combobox", { name: "Select repository" });
  fireEvent.keyDown(picker, { key: "ArrowDown" });
  expect(pickerOptionRender).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(picker, { key: "ArrowDown" });
  expect(pickerOptionRender).toHaveBeenCalledTimes(3);
});

it("projects repository loading, empty, and error states through ResourceStateNotice", () => {
  useWorkbenchStore.setState({
    repositoryRefs: [],
    repositoryInventoryPhase: "idle",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  const { rerender } = render(<SourceControlView />);
  expect(screen.queryByText("No repositories found")).not.toBeInTheDocument();

  useWorkbenchStore.setState({ repositoryInventoryPhase: "loading" });
  rerender(<SourceControlView />);
  expect(screen.getByRole("status")).toHaveTextContent(
    "Loading repository status",
  );

  useWorkbenchStore.setState({ repositoryInventoryPhase: "ready" });
  rerender(<SourceControlView />);
  expect(screen.getByRole("status")).toHaveTextContent("No repositories found");

  useWorkbenchStore.setState({
    repositoryInventoryError: "Could not refresh repository inventory.",
  });
  rerender(<SourceControlView />);
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Could not refresh repository inventory.",
  );

  useWorkbenchStore.setState({
    repositoryInventoryError: null,
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    repositoryInventoryPhase: "ready",
  });
  rerender(<SourceControlView />);
  expect(
    screen.getByRole("button", { name: "Select repository" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("No repositories found")).not.toBeInTheDocument();
});

it("shows native-proven identity and dispatches the selected publish action", async () => {
  const pushRemote = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: {
      roots: [{ ...rootStatus, upstream: null, ahead: 0, behind: 0 }],
      truncated: false,
      elapsedMs: 1,
    },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    pushRemote,
  });
  render(<SourceControlView />);
  expect(
    screen.getByRole("group", {
      name: "Root A repository, branch main, no upstream, 0 ahead, 0 behind",
    }),
  ).toBeVisible();
  expect(screen.getByTitle("Branch main")).toBeVisible();
  expect(screen.getByTitle("No upstream · 0 ahead, 0 behind")).toBeVisible();
  expect(screen.getByText("No upstream")).toBeVisible();
  expect(
    screen.queryByRole("form", {
      name: "Commit staged changes in Root A",
    }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Publish Branch" }));
  await waitFor(() =>
    expect(pushRemote).toHaveBeenCalledWith("root-a", "origin"),
  );
});

it("dispatches Pull for the native-proven behind primary action", async () => {
  const pullRepository = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: {
      roots: [
        {
          ...rootStatus,
          upstream: "origin/main",
          ahead: 0,
          behind: 1,
          changes: [],
          truncated: false,
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    pullRepository,
  });
  render(<SourceControlView />);

  expect(
    screen.queryByRole("form", {
      name: "Commit staged changes in Root A",
    }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Pull" }));
  await waitFor(() => expect(pullRepository).toHaveBeenCalledWith("root-a"));
});

it("dispatches Push to the remote proven by the current upstream", async () => {
  const pushRemote = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: {
      roots: [
        { ...rootStatus, upstream: "upstream/main", ahead: 2, behind: 0 },
      ],
      truncated: false,
      elapsedMs: 1,
    },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    pushRemote,
  });
  render(<SourceControlView />);

  expect(screen.getByText("↑2 ↓0")).toBeVisible();
  expect(
    screen.queryByRole("form", {
      name: "Commit staged changes in Root A",
    }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Push" }));
  await waitFor(() =>
    expect(pushRemote).toHaveBeenCalledWith("root-a", "upstream"),
  );
});

it("explains why a diverged branch cannot be synced automatically", () => {
  const reason =
    "This branch has diverged from its upstream. Sage cannot yet choose a merge or rebase safely.";
  useWorkbenchStore.setState({
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
    repositories: {
      roots: [{ ...rootStatus, upstream: "origin/main", ahead: 2, behind: 1 }],
      truncated: false,
      elapsedMs: 1,
    },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<SourceControlView />);

  const sync = screen.getByRole("button", { name: "Sync Changes" });
  expect(sync).toBeDisabled();
  expect(sync).toHaveAccessibleDescription(reason);
  expect(screen.getByText(reason)).toBeVisible();
  expect(
    screen.queryByRole("form", {
      name: "Commit staged changes in Root A",
    }),
  ).toBeNull();
});

it("selects a nested repository, opens its Diff, and offers no mutations", async () => {
  const selectRepository = vi.fn(async (repository: RepositoryRef) => {
    useWorkbenchStore.setState({
      selectedRepository: repository,
      selectedRepositoryStatus: nestedStatus,
    });
  });
  const openDiff = vi.fn(async () => undefined);

  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository, nestedRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    selectedResource: nestedStatus.changes[0].resource,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    selectRepository,
    openDiff,
    mutateRepositoryFile: vi.fn(async () => true),
  });

  render(<SourceControlView />);

  fireEvent.click(screen.getByRole("button", { name: "Select repository" }));
  const repositoryPicker = screen.getByRole("combobox", {
    name: "Select repository",
  });
  fireEvent.keyDown(repositoryPicker, { key: "ArrowDown" });
  fireEvent.keyDown(repositoryPicker, { key: "ArrowDown" });
  fireEvent.keyDown(repositoryPicker, { key: "Enter" });

  await waitFor(() =>
    expect(selectRepository).toHaveBeenCalledWith(nestedRepository),
  );

  const selectedChange = screen.getByRole("button", {
    name: "Open diff for packages/tool/src/app.ts",
  });
  expect(selectedChange).toHaveAttribute("aria-current", "true");
  fireEvent.click(selectedChange);

  expect(openDiff).toHaveBeenCalledWith(
    nestedStatus.changes[0].resource,
    nestedRepository,
  );
  expect(
    screen.queryByRole("button", {
      name: "Unstage packages/tool/src/app.ts in Root A",
    }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", {
      name: "Stage packages/tool/src/app.ts in Root A",
    }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", {
      name: "Discard unstaged changes to packages/tool/src/app.ts in Root A",
    }),
  ).toBeNull();
});

it("does not show root changes while nested status is loading", () => {
  useWorkbenchStore.setState({
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
    repositories: {
      roots: [
        {
          ...rootStatus,
          changes: [
            {
              resource: { rootId: "root-a", relativePath: "root-only.ts" },
              indexStatus: "modified",
              worktreeStatus: "modified",
              additions: 1,
              deletions: 0,
            },
          ],
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
    repositoryRefs: [rootRepository, nestedRepository],
    selectedRepository: nestedRepository,
    selectedRepositoryStatus: null,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "loading",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  expect(
    screen.queryByRole("button", { name: "Open diff for root-only.ts" }),
  ).toBeNull();
});

it("does not mark a local branch current while HEAD is detached", () => {
  const detached: RootRepositoryStatus = {
    ...rootStatus,
    branch: "01234567 (detached)",
  };
  useWorkbenchStore.setState({
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
    repositories: { roots: [detached], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: detached,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  const list = screen.getByRole("list", { name: "Local branches in Root A" });
  expect(list).toHaveTextContent("main");
  expect(list).toHaveTextContent("topic");
  expect(list).not.toHaveTextContent("(current)");
});

it("lists local branches and marks the current named branch", () => {
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  const list = screen.getByRole("list", { name: "Local branches in Root A" });
  expect(list).toHaveTextContent("main (current)");
  expect(list).toHaveTextContent("topic");
  expect(list).not.toHaveTextContent("topic (current)");
});

it("lists recent commits newest first with a shortened SHA", () => {
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  const list = screen.getByRole("list", { name: "Recent commits in Root A" });
  expect(list.querySelectorAll(".source-commit__sha")[0]).toHaveTextContent(
    "aaaaaaa",
  );
  expect(list.querySelectorAll(".source-commit__subject")[0]).toHaveTextContent(
    "second",
  );
  expect(list.querySelectorAll(".source-commit__sha")[1]).toHaveTextContent(
    "bbbbbbb",
  );
  expect(list).not.toHaveTextContent(
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  );
  const remotes = screen.getByRole("list", { name: "Remotes in Root A" });
  expect(remotes).toHaveTextContent("origin");
  expect(remotes).toHaveTextContent("upstream");
  expect(remotes).not.toHaveTextContent("http");
  expect(remotes).not.toHaveTextContent(".git");
});

it("places the commit control before repository detail lists", () => {
  const stagedStatus: RootRepositoryStatus = {
    ...rootStatus,
    changes: [
      {
        resource: { rootId: "root-a", relativePath: "src/app.ts" },
        indexStatus: "modified",
        worktreeStatus: "unchanged",
        additions: 1,
        deletions: 0,
      },
    ],
  };
  useWorkbenchStore.setState({
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
    repositories: { roots: [stagedStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: stagedStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  const commitControl = screen.getByRole("form", {
    name: "Commit staged changes in Root A",
  });
  const branchList = screen.getByRole("list", {
    name: "Local branches in Root A",
  });

  expect(
    commitControl.compareDocumentPosition(branchList) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

it("distinguishes repository errors from loading and informational states", () => {
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    repositoryInventoryError: "Repository inventory failed.",
    repositoryCheckoutError: {
      rootId: "root-a",
      branch: "topic",
      kind: "failed",
      message: "Checkout failed.",
    },
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  expect(screen.getAllByRole("alert")).toHaveLength(2);
  expect(
    screen.getByText("Repository inventory failed.").closest('[role="alert"]'),
  ).toHaveClass("resource-state--error");
  expect(screen.getByText("Checkout failed.")).toHaveClass("sidebar__error");
});

it("lists conflicted files under a Conflicts heading with a Conflict chip", () => {
  useWorkbenchStore.setState({
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
    repositories: {
      roots: [
        {
          ...rootStatus,
          changes: [
            {
              resource: { rootId: "root-a", relativePath: "ok.ts" },
              indexStatus: "modified",
              worktreeStatus: "unchanged",
              additions: 1,
              deletions: 0,
            },
            {
              resource: { rootId: "root-a", relativePath: "bad.ts" },
              indexStatus: "conflicted",
              worktreeStatus: "conflicted",
              additions: null,
              deletions: null,
            },
          ],
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: {
      ...rootStatus,
      changes: [
        {
          resource: { rootId: "root-a", relativePath: "ok.ts" },
          indexStatus: "modified",
          worktreeStatus: "unchanged",
          additions: 1,
          deletions: 0,
        },
        {
          resource: { rootId: "root-a", relativePath: "bad.ts" },
          indexStatus: "conflicted",
          worktreeStatus: "conflicted",
          additions: null,
          deletions: null,
        },
      ],
    },
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  expect(screen.getByText("Conflicts")).toBeInTheDocument();
  expect(screen.getByText("Conflict")).toBeInTheDocument();
  const bad = screen.getByText("bad.ts");
  const ok = screen.getByText("ok.ts");
  expect(
    bad.compareDocumentPosition(ok) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Keep ours for bad.ts in Root A" }),
  ).toBeInTheDocument();
});

it("lists rebase conflicts under a Rebase heading", () => {
  const rebasing: RootRepositoryStatus = {
    ...rootStatus,
    rebaseInProgress: true,
    changes: [
      {
        resource: { rootId: "root-a", relativePath: "bad.ts" },
        indexStatus: "conflicted",
        worktreeStatus: "conflicted",
        additions: null,
        deletions: null,
      },
    ],
  };
  useWorkbenchStore.setState({
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
    repositories: { roots: [rebasing], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rebasing,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  expect(screen.getByText("Rebase")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Publish Branch" })).toBeNull();
  expect(screen.queryByText("Conflicts")).toBeNull();
  expect(
    screen.getByRole("button", { name: "Keep ours for bad.ts in Root A" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("Commit merge")).toBeNull();
  expect(
    screen.getByRole("button", { name: "Continue rebase in Root A" }),
  ).toBeDisabled();
  expect(screen.queryByText("Continue rebase stays later.")).toBeNull();
});

it("lists cherry-pick conflicts under a Cherry-pick heading", () => {
  const cherryPicking: RootRepositoryStatus = {
    ...rootStatus,
    cherryPickInProgress: true,
    changes: [
      {
        resource: { rootId: "root-a", relativePath: "bad.ts" },
        indexStatus: "conflicted",
        worktreeStatus: "conflicted",
        additions: null,
        deletions: null,
      },
    ],
  };
  useWorkbenchStore.setState({
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
    repositories: { roots: [cherryPicking], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: cherryPicking,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  expect(screen.getByText("Cherry-pick")).toBeInTheDocument();
  expect(screen.queryByText("Conflicts")).toBeNull();
  expect(screen.queryByText("Rebase")).toBeNull();
  expect(
    screen.getByRole("button", { name: "Keep ours for bad.ts in Root A" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("Commit merge")).toBeNull();
  expect(
    screen.getByRole("button", { name: "Continue cherry-pick in Root A" }),
  ).toBeDisabled();
});

it("lists revert conflicts under a Revert heading", () => {
  const reverting: RootRepositoryStatus = {
    ...rootStatus,
    revertInProgress: true,
    changes: [
      {
        resource: { rootId: "root-a", relativePath: "bad.ts" },
        indexStatus: "conflicted",
        worktreeStatus: "conflicted",
        additions: null,
        deletions: null,
      },
    ],
  };
  useWorkbenchStore.setState({
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
    repositories: { roots: [reverting], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: reverting,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  expect(screen.getByText("Revert")).toBeInTheDocument();
  expect(screen.queryByText("Conflicts")).toBeNull();
  expect(screen.queryByText("Cherry-pick")).toBeNull();
  expect(
    screen.getByRole("button", { name: "Keep ours for bad.ts in Root A" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("Commit merge")).toBeNull();
  expect(
    screen.getByRole("button", { name: "Continue revert in Root A" }),
  ).toBeDisabled();
});

it("keeps ours for a conflicted file through the mutation port", async () => {
  const mutateRepositoryFile = vi.fn(async () => true);
  const conflicted: RootRepositoryStatus = {
    ...rootStatus,
    changes: [
      {
        resource: { rootId: "root-a", relativePath: "bad.ts" },
        indexStatus: "conflicted",
        worktreeStatus: "conflicted",
        additions: null,
        deletions: null,
      },
    ],
  };
  useWorkbenchStore.setState({
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
    repositories: { roots: [conflicted], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: conflicted,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    mutateRepositoryFile,
  });

  render(<SourceControlView />);
  fireEvent.click(
    screen.getByRole("button", { name: "Keep ours for bad.ts in Root A" }),
  );
  await waitFor(() =>
    expect(mutateRepositoryFile).toHaveBeenCalledWith(
      conflicted.changes[0].resource,
      "keepOurs",
    ),
  );
});

it("offers a switch control for a listed non-current local branch", async () => {
  const checkoutLocalBranch = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    checkoutLocalBranch,
  });

  render(<SourceControlView />);

  fireEvent.click(screen.getByRole("button", { name: "Switch to topic" }));
  await waitFor(() => {
    expect(checkoutLocalBranch).toHaveBeenCalledWith("root-a", "topic");
  });
  expect(
    screen.queryByRole("button", { name: "Switch to main" }),
  ).not.toBeInTheDocument();
});

it("offers a New branch form that creates an unlisted valid name without switching", async () => {
  const createLocalBranch = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    createLocalBranch,
  });

  render(<SourceControlView />);

  fireEvent.change(screen.getByRole("textbox", { name: "New branch name" }), {
    target: { value: "feature" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Create branch" }));
  await waitFor(() => {
    expect(createLocalBranch).toHaveBeenCalledWith("root-a", "feature");
  });
  expect(createLocalBranch).toHaveBeenCalledTimes(1);
});

it("refuses Create branch for a listed or unsafe name", () => {
  const createLocalBranch = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    createLocalBranch,
  });

  render(<SourceControlView />);

  const create = screen.getByRole("button", { name: "Create branch" });
  expect(create).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "New branch name" }), {
    target: { value: "main" },
  });
  expect(create).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "New branch name" }), {
    target: { value: "-bad" },
  });
  expect(create).toBeDisabled();
  fireEvent.click(create);
  expect(createLocalBranch).not.toHaveBeenCalled();
});

it("offers Delete for a listed non-current local branch and hides it for current", async () => {
  const deleteLocalBranch = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    deleteLocalBranch,
  });

  render(<SourceControlView />);

  fireEvent.click(screen.getByRole("button", { name: "Delete topic" }));
  await waitFor(() => {
    expect(deleteLocalBranch).toHaveBeenCalledWith("root-a", "topic");
  });
  expect(
    screen.queryByRole("button", { name: "Delete main" }),
  ).not.toBeInTheDocument();
});

it("offers Rename branch for a listed name to a valid unlisted name", async () => {
  const renameLocalBranch = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    renameLocalBranch,
  });

  render(<SourceControlView />);

  fireEvent.change(screen.getByRole("combobox", { name: "Branch to rename" }), {
    target: { value: "topic" },
  });
  fireEvent.change(
    screen.getByRole("textbox", { name: "Renamed branch name" }),
    { target: { value: "feature" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Rename branch" }));
  await waitFor(() => {
    expect(renameLocalBranch).toHaveBeenCalledWith(
      "root-a",
      "topic",
      "feature",
    );
  });
  expect(renameLocalBranch).toHaveBeenCalledTimes(1);
});

it("refuses Rename branch for a listed or unsafe destination", () => {
  const renameLocalBranch = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    renameLocalBranch,
  });

  render(<SourceControlView />);

  const rename = screen.getByRole("button", { name: "Rename branch" });
  expect(rename).toBeDisabled();
  fireEvent.change(
    screen.getByRole("textbox", { name: "Renamed branch name" }),
    { target: { value: "main" } },
  );
  expect(rename).toBeDisabled();
  fireEvent.change(
    screen.getByRole("textbox", { name: "Renamed branch name" }),
    { target: { value: "-bad" } },
  );
  expect(rename).toBeDisabled();
  fireEvent.click(rename);
  expect(renameLocalBranch).not.toHaveBeenCalled();
});

it("offers fetch and push on listed remote names without URLs", async () => {
  const fetchRemote = vi.fn(async () => true);
  const pushRemote = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [rootStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: rootStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    fetchRemote,
    pushRemote,
  });

  render(<SourceControlView />);

  fireEvent.click(screen.getByRole("button", { name: "Fetch origin" }));
  await waitFor(() => {
    expect(fetchRemote).toHaveBeenCalledWith("root-a", "origin");
  });
  fireEvent.click(screen.getByRole("button", { name: "Push upstream" }));
  await waitFor(() => {
    expect(pushRemote).toHaveBeenCalledWith("root-a", "upstream");
  });
  expect(screen.queryByText(/https?:\/\//)).not.toBeInTheDocument();
});

it("uses the live selected root status after staging a selected root file", async () => {
  const unstagedStatus: RootRepositoryStatus = {
    ...rootStatus,
    changes: [
      {
        resource: { rootId: "root-a", relativePath: "src/app.ts" },
        indexStatus: "unchanged",
        worktreeStatus: "modified",
        additions: 1,
        deletions: 0,
      },
    ],
  };
  const stagedStatus: RootRepositoryStatus = {
    ...unstagedStatus,
    changes: [
      {
        ...unstagedStatus.changes[0],
        indexStatus: "modified",
        worktreeStatus: "unchanged",
      },
    ],
  };
  const mutateRepositoryFile = vi.fn(async () => {
    useWorkbenchStore.setState({
      repositories: { roots: [stagedStatus], truncated: false, elapsedMs: 1 },
    });
    return true;
  });

  useWorkbenchStore.setState({
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
    repositories: { roots: [unstagedStatus], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: unstagedStatus,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    mutateRepositoryFile,
  });

  render(<SourceControlView />);

  expect(
    screen.getByRole("button", { name: "Stage src/app.ts in Root A" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Publish Branch" })).toBeEnabled();
  expect(
    screen.queryByRole("form", { name: "Commit staged changes in Root A" }),
  ).toBeNull();

  fireEvent.click(
    screen.getByRole("button", { name: "Stage src/app.ts in Root A" }),
  );

  await waitFor(() =>
    expect(mutateRepositoryFile).toHaveBeenCalledWith(
      unstagedStatus.changes[0].resource,
      "stage",
    ),
  );

  await waitFor(() => {
    expect(
      screen.queryByRole("button", { name: "Stage src/app.ts in Root A" }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "Unstage src/app.ts in Root A" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Publish Branch" })).toBeNull();
    expect(
      screen.getByRole("button", { name: "Commit staged changes in Root A" }),
    ).toBeDisabled();
  });
  fireEvent.change(screen.getByLabelText("Commit message for Root A"), {
    target: { value: "Stage selected root" },
  });
  expect(
    screen.getByRole("button", { name: "Commit staged changes in Root A" }),
  ).toBeEnabled();
});

it("enables Commit merge after a resolved merge with no leftover staged delta", () => {
  const resolvedMerge: RootRepositoryStatus = {
    ...rootStatus,
    mergeInProgress: true,
    changes: [],
  };
  useWorkbenchStore.setState({
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
    repositories: { roots: [resolvedMerge], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: resolvedMerge,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<SourceControlView />);

  fireEvent.change(screen.getByLabelText("Commit message for Root A"), {
    target: { value: "Merge other" },
  });
  expect(
    screen.getByRole("button", { name: "Commit merge in Root A" }),
  ).toBeEnabled();
  expect(
    screen.getByText("This completes the in-progress merge."),
  ).toBeInTheDocument();
});

it("enables Continue rebase after a resolved rebase with no leftover conflicts", () => {
  const resolvedRebase: RootRepositoryStatus = {
    ...rootStatus,
    rebaseInProgress: true,
    changes: [],
  };
  const continueRebase = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [resolvedRebase], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: resolvedRebase,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    continueRebase,
  });

  render(<SourceControlView />);

  expect(screen.queryByText("Commit merge")).toBeNull();
  expect(screen.queryByRole("button", { name: "Publish Branch" })).toBeNull();
  expect(screen.queryByText("Continue rebase stays later.")).toBeNull();
  expect(screen.getByText("Conflicts are resolved.")).toBeInTheDocument();
  expect(
    screen.getByText("This continues the in-progress rebase."),
  ).toBeInTheDocument();
  const continueButton = screen.getByRole("button", {
    name: "Continue rebase in Root A",
  });
  expect(continueButton).toBeEnabled();
  fireEvent.click(continueButton);
  expect(continueRebase).toHaveBeenCalledWith("root-a");
});

it("offers Abort rebase while conflicts remain and Continue rebase is disabled", () => {
  const conflictedRebase: RootRepositoryStatus = {
    ...rootStatus,
    rebaseInProgress: true,
    changes: [
      {
        resource: { rootId: "root-a", relativePath: "conflict.txt" },
        indexStatus: "conflicted",
        worktreeStatus: "conflicted",
        additions: null,
        deletions: null,
      },
    ],
  };
  const abortInProgress = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [conflictedRebase], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: conflictedRebase,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    abortInProgress,
  });

  render(<SourceControlView />);

  expect(
    screen.getByRole("button", { name: "Continue rebase in Root A" }),
  ).toBeDisabled();
  const abort = screen.getByRole("button", {
    name: "Abort rebase in Root A",
  });
  expect(abort).toBeEnabled();
  fireEvent.click(abort);
  expect(abortInProgress).toHaveBeenCalledWith("root-a");
});

it("offers Abort merge during an exclusive merge even with leftover conflicts", () => {
  const conflictedMerge: RootRepositoryStatus = {
    ...rootStatus,
    mergeInProgress: true,
    changes: [
      {
        resource: { rootId: "root-a", relativePath: "conflict.txt" },
        indexStatus: "conflicted",
        worktreeStatus: "conflicted",
        additions: null,
        deletions: null,
      },
    ],
  };
  const abortInProgress = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [conflictedMerge], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: conflictedMerge,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    abortInProgress,
  });

  render(<SourceControlView />);

  const abort = screen.getByRole("button", {
    name: "Abort merge in Root A",
  });
  expect(abort).toBeEnabled();
  fireEvent.click(abort);
  expect(abortInProgress).toHaveBeenCalledWith("root-a");
});

it("enables Continue cherry-pick after a resolved cherry-pick with no leftover conflicts", () => {
  const resolvedCherryPick: RootRepositoryStatus = {
    ...rootStatus,
    cherryPickInProgress: true,
    changes: [],
  };
  const continueCherryPick = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: {
      roots: [resolvedCherryPick],
      truncated: false,
      elapsedMs: 1,
    },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: resolvedCherryPick,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    continueCherryPick,
  });

  render(<SourceControlView />);

  expect(screen.queryByText("Commit merge")).toBeNull();
  expect(screen.queryByText("Continue rebase")).toBeNull();
  expect(screen.getByText("Conflicts are resolved.")).toBeInTheDocument();
  expect(
    screen.getByText("This continues the in-progress cherry-pick."),
  ).toBeInTheDocument();
  const continueButton = screen.getByRole("button", {
    name: "Continue cherry-pick in Root A",
  });
  expect(continueButton).toBeEnabled();
  fireEvent.click(continueButton);
  expect(continueCherryPick).toHaveBeenCalledWith("root-a");
});

it("enables Continue revert after a resolved revert with no leftover conflicts", () => {
  const resolvedRevert: RootRepositoryStatus = {
    ...rootStatus,
    revertInProgress: true,
    changes: [],
  };
  const continueRevert = vi.fn(async () => true);
  useWorkbenchStore.setState({
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
    repositories: { roots: [resolvedRevert], truncated: false, elapsedMs: 1 },
    repositoryRefs: [rootRepository],
    selectedRepository: rootRepository,
    selectedRepositoryStatus: resolvedRevert,
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    continueRevert,
  });

  render(<SourceControlView />);

  expect(screen.queryByText("Commit merge")).toBeNull();
  expect(screen.queryByText("Continue cherry-pick")).toBeNull();
  expect(screen.getByText("Conflicts are resolved.")).toBeInTheDocument();
  expect(
    screen.getByText("This continues the in-progress revert."),
  ).toBeInTheDocument();
  const continueButton = screen.getByRole("button", {
    name: "Continue revert in Root A",
  });
  expect(continueButton).toBeEnabled();
  fireEvent.click(continueButton);
  expect(continueRevert).toHaveBeenCalledWith("root-a");
});
