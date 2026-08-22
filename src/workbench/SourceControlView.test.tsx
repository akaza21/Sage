import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { repositoryKey } from "../domain/repositoryInventory";
import type { RepositoryRef, RootRepositoryStatus } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { SourceControlView } from "./SourceControlView";

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

  expect(
    screen.getByRole("option", { name: "Workspace (root 1) — packages/tool" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("option", { name: "Workspace (root 2) — packages/tool" }),
  ).toBeInTheDocument();
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
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: vi.fn(async () => undefined),
    selectRepository,
    openDiff,
    mutateRepositoryFile: vi.fn(async () => true),
  });

  render(<SourceControlView />);

  fireEvent.change(
    screen.getByRole("combobox", { name: "Select repository" }),
    {
      target: { value: repositoryKey(nestedRepository) },
    },
  );

  await waitFor(() =>
    expect(selectRepository).toHaveBeenCalledWith(nestedRepository),
  );

  fireEvent.click(
    screen.getByRole("button", {
      name: "Open diff for packages/tool/src/app.ts",
    }),
  );

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
  expect(
    screen.getByRole("button", { name: "Commit staged changes in Root A" }),
  ).toBeDisabled();

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
  });
  fireEvent.change(screen.getByLabelText("Commit message for Root A"), {
    target: { value: "Stage selected root" },
  });
  expect(
    screen.getByRole("button", { name: "Commit staged changes in Root A" }),
  ).toBeEnabled();
});
