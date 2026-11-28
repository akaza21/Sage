import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type {
  RepositoryRef,
  RepositorySummary,
  RootRepositoryStatus,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { SourceControlView } from "./SourceControlView";

const initialState = useWorkbenchStore.getState();
const api: RepositoryRef = { rootId: "root-a", relativePath: "api" };
const web: RepositoryRef = { rootId: "root-a", relativePath: "web" };

function summary(
  ref: RepositoryRef,
  branch: string,
  changeCount: number,
): RepositorySummary {
  return {
    ref,
    state: "repository",
    branch,
    changeCount,
    changesOverflowed: false,
    message: null,
  };
}

function nestedStatus(
  ref: RepositoryRef,
  files: string[],
): RootRepositoryStatus {
  return {
    rootId: ref.rootId,
    state: "repository",
    branch: "main",
    changes: files.map((file) => ({
      resource: {
        rootId: ref.rootId,
        relativePath: `${ref.relativePath}/${file}`,
      },
      indexStatus: "unchanged",
      worktreeStatus: "modified",
      additions: 1,
      deletions: 1,
    })),
    truncated: false,
    elapsedMs: 1,
    message: null,
  };
}

function openFolderOfRepositories(options: { truncated?: boolean } = {}) {
  const repository = getWorkbenchPorts().repository;
  const discover = vi
    .spyOn(repository, "discoverProjectRepositories")
    .mockResolvedValue({
      projectId: "project-a",
      transitionEpoch: 0,
      refs: [api, web],
      summaries: [summary(api, "main", 2), summary(web, "topic", 1)],
      truncated: options.truncated ?? false,
    });
  vi.spyOn(repository, "getNestedRepositoryStatus").mockImplementation(
    async (request) => ({
      projectId: "project-a",
      transitionEpoch: 0,
      ref: request.ref,
      status:
        request.ref.relativePath === "api"
          ? nestedStatus(api, ["a.ts", "b.ts"])
          : nestedStatus(web, ["page.tsx"]),
    }),
  );
  useWorkbenchStore.setState({
    phase: "ready",
    nativeTransitionEpoch: 0,
    projectGeneration: 0,
    projectTransitioning: false,
    project: {
      id: "project-a",
      name: "multi",
      roots: [
        {
          id: "root-a",
          name: "multi",
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
          state: "notRepository",
          branch: null,
          changes: [],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
  });
  return discover;
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

it("lists each repository of a folder with its branch and change count, and opens one to its changes", async () => {
  openFolderOfRepositories();
  const openDiff = vi.fn(async () => undefined);
  useWorkbenchStore.setState({ openDiff });
  render(<SourceControlView />);

  const list = await screen.findByRole("list", { name: "Repositories" });
  const apiRow = within(list).getByRole("button", {
    name: "api, main, 2 changes",
  });
  const webRow = within(list).getByRole("button", {
    name: "web, topic, 1 change",
  });
  expect(
    screen.queryByRole("button", { name: "Select repository" }),
  ).toBeNull();
  await waitFor(() => expect(apiRow).toHaveAttribute("aria-current", "true"));
  expect(
    (
      await screen.findAllByRole("button", { name: "Open diff for api/a.ts" })
    )[0],
  ).toBeVisible();
  expect(
    screen.queryAllByRole("button", { name: "Open diff for web/page.tsx" }),
  ).toHaveLength(0);

  fireEvent.click(webRow);

  const webChange = (
    await screen.findAllByRole("button", { name: "Open diff for web/page.tsx" })
  )[0];
  expect(webRow).toHaveAttribute("aria-current", "true");
  expect(apiRow).not.toHaveAttribute("aria-current");
  expect(
    screen.queryAllByRole("button", { name: "Open diff for api/a.ts" }),
  ).toHaveLength(0);
  fireEvent.click(webChange);
  expect(openDiff).toHaveBeenCalledWith(
    { rootId: "root-a", relativePath: "web/page.tsx" },
    web,
  );
});

it("says when more repositories exist than the list shows", async () => {
  openFolderOfRepositories({ truncated: true });
  render(<SourceControlView />);
  expect(await screen.findByText("More repositories not shown.")).toBeVisible();
});

it("re-reads the folder's repositories on window focus without blanking the shown changes", async () => {
  const discover = openFolderOfRepositories();
  render(<SourceControlView />);
  await screen.findAllByRole("button", { name: "Open diff for api/a.ts" });
  discover.mockResolvedValue({
    projectId: "project-a",
    transitionEpoch: 0,
    refs: [api, web],
    summaries: [summary(api, "main", 3), summary(web, "topic", 1)],
    truncated: false,
  });

  act(() => {
    window.dispatchEvent(new Event("focus"));
  });

  expect(screen.queryByRole("status")).toBeNull();
  expect(
    screen.getAllByRole("button", { name: "Open diff for api/a.ts" })[0],
  ).toBeVisible();
  expect(
    await screen.findByRole("button", { name: "api, main, 3 changes" }),
  ).toHaveAttribute("aria-current", "true");
});
