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
  useWorkbenchStore.setState(initialState, true);
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
