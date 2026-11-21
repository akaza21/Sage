import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import type {
  GitFileMutationRequest,
  GitFileMutationResult,
  GitPathsMutationRequest,
  RootRepositoryStatus,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { SourceControlView } from "./SourceControlView";
import { StatusBar } from "./StatusBar";
import { preloadSearchablePicker } from "../ui/searchablePickerLoad";

const initial = useWorkbenchStore.getState();
const root: RootRepositoryStatus = {
  rootId: "root-a",
  state: "repository",
  branch: "main",
  branches: ["main", "topic"],
  remotes: ["origin"],
  changes: [
    {
      resource: { rootId: "root-a", relativePath: "src/app.ts" },
      indexStatus: "unchanged",
      worktreeStatus: "modified",
      additions: 3,
      deletions: 1,
    },
  ],
  truncated: false,
  elapsedMs: 1,
  message: null,
};

beforeEach(async () => {
  await preloadSearchablePicker();
  useWorkbenchStore.setState({
    phase: "ready",
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
      roots: [structuredClone(root)],
      truncated: false,
      elapsedMs: 1,
    },
    repositoryRefs: [{ rootId: "root-a", relativePath: "" }],
    selectedRepository: { rootId: "root-a", relativePath: "" },
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
    loadRepositoryInventory: async () => undefined,
    loadEditor: async () => undefined,
    revealFile: async () => undefined,
  });
  const mutationResponse = async (
    request:
      | GitFileMutationRequest
      | (GitPathsMutationRequest & { action: "stage" | "unstage" | "discard" }),
    epoch: number,
  ): Promise<GitFileMutationResult> => {
    const current = useWorkbenchStore.getState().repositories.roots[0];
    const paths =
      "resources" in request ? request.resources : [request.resource];
    const selected = (path: string) =>
      paths.some((resource) => resource.relativePath === path);
    return {
      projectId: request.projectId,
      transitionEpoch: epoch!,
      error: null,
      repositories: {
        roots: [
          {
            ...current,
            changes:
              request.action === "discard"
                ? current.changes.flatMap((change) =>
                    !selected(change.resource.relativePath)
                      ? [change]
                      : change.indexStatus === "unchanged"
                        ? []
                        : [{ ...change, worktreeStatus: "unchanged" as const }],
                  )
                : current.changes.map((change) =>
                    !selected(change.resource.relativePath)
                      ? change
                      : {
                          ...change,
                          indexStatus:
                            request.action === "stage"
                              ? "modified"
                              : "unchanged",
                          worktreeStatus:
                            request.action === "stage"
                              ? "unchanged"
                              : "modified",
                        },
                  ),
          },
        ],
        truncated: false,
        elapsedMs: 1,
      },
    };
  };
  vi.spyOn(desktop, "mutateGitFile").mockImplementation(mutationResponse);
  vi.spyOn(desktop, "stageGitPaths").mockImplementation((request, epoch) =>
    mutationResponse({ ...request, action: "stage" }, epoch),
  );
  vi.spyOn(desktop, "unstageGitPaths").mockImplementation((request, epoch) =>
    mutationResponse({ ...request, action: "unstage" }, epoch),
  );
  vi.spyOn(desktop, "discardGitPaths").mockImplementation((request, epoch) =>
    mutationResponse({ ...request, action: "discard" }, epoch),
  );
  vi.spyOn(desktop, "commitGitRepository").mockImplementation(
    async (request, epoch) => {
      const current = useWorkbenchStore.getState().repositories.roots[0];
      return {
        projectId: request.projectId,
        rootId: request.rootId,
        transitionEpoch: epoch!,
        error: null,
        repositories: {
          roots: [
            {
              ...current,
              changes: [],
              recentCommits: [
                { sha: "a".repeat(40), subject: request.message },
              ],
            },
          ],
          truncated: false,
          elapsedMs: 1,
        },
      };
    },
  );
});

it("commits staged changes with Ctrl+Enter and exposes only the supported commit variant", async () => {
  useWorkbenchStore.setState({
    repositories: {
      roots: [
        {
          ...root,
          changes: [
            {
              ...root.changes[0],
              indexStatus: "modified",
              worktreeStatus: "unchanged",
            },
          ],
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
  });
  render(<SourceControlView />);
  const message = screen.getByLabelText("Commit message for Root A");
  fireEvent.change(message, { target: { value: "Staged commit" } });
  fireEvent.click(screen.getByRole("button", { name: "Commit actions" }));
  expect(
    screen.getAllByRole("option").map((option) => option.textContent),
  ).toEqual(["Commit staged changes"]);
  fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
  fireEvent.keyDown(message, { key: "Enter", ctrlKey: true });
  await waitFor(() => expect(message).toHaveValue(""));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(
    useWorkbenchStore.getState().repositories.roots[0].recentCommits?.[0]
      .subject,
  ).toBe("Staged commit");
});

it("stages from the header, unstages from the staged row and group, and hides empty groups", async () => {
  render(<SourceControlView />);
  expect(screen.queryByText("Staged Changes")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Select repository" }),
  ).not.toBeInTheDocument();
  expect(screen.queryByText("Root A")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Stage All Changes" }));
  await waitFor(() => expect(screen.getByText("Staged Changes")).toBeVisible());
  expect(
    screen.queryByRole("button", { name: "Stage src/app.ts in Root A" }),
  ).not.toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "Unstage src/app.ts in Root A" }),
  );
  await waitFor(() =>
    expect(screen.queryByText("Staged Changes")).not.toBeInTheDocument(),
  );
  fireEvent.click(screen.getByRole("button", { name: "Stage changes" }));
  await waitFor(() => expect(screen.getByText("Staged Changes")).toBeVisible());
  fireEvent.click(screen.getByRole("button", { name: "Unstage All Changes" }));
  await waitFor(() =>
    expect(screen.queryByText("Staged Changes")).not.toBeInTheDocument(),
  );
  expect(useWorkbenchStore.getState().repositories.roots[0].changes).toEqual(
    root.changes,
  );
});

it("keeps conflicts intact through stage, unstage and discard group actions", async () => {
  const conflict = {
    ...root.changes[0],
    resource: { rootId: root.rootId, relativePath: "conflict.txt" },
    indexStatus: "conflicted" as const,
    worktreeStatus: "conflicted" as const,
  };
  useWorkbenchStore.setState({
    repositories: {
      roots: [{ ...root, changes: [conflict, ...root.changes] }],
      truncated: false,
      elapsedMs: 1,
    },
  });
  render(<SourceControlView />);
  fireEvent.click(screen.getByRole("button", { name: "Stage All Changes" }));
  await waitFor(() => expect(screen.getByText("Staged Changes")).toBeVisible());
  expect(useWorkbenchStore.getState().repositories.roots[0].changes[0]).toEqual(
    conflict,
  );
  fireEvent.click(screen.getByRole("button", { name: "Unstage All Changes" }));
  await waitFor(() =>
    expect(screen.queryByText("Staged Changes")).not.toBeInTheDocument(),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "More Source Control Actions" }),
  );
  fireEvent.click(
    screen.getByRole("menuitem", { name: /Discard All Changes/ }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Discard All Changes" }));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().repositories.roots[0].changes).toEqual([
      conflict,
    ]),
  );
  expect(
    screen.queryByRole("button", { name: /^(Stage|Unstage|Discard) / }),
  ).not.toBeInTheDocument();
});

it("renders file name, folder and status without line counts; opens the File and Diff resources", async () => {
  render(<SourceControlView />);
  const row = screen.getByRole("button", { name: "Open diff for src/app.ts" });
  expect(within(row).getByText("app.ts")).toBeVisible();
  expect(within(row).getByText("src")).toBeVisible();
  expect(row.querySelector(".file-kind-icon--typescript")).toBeInTheDocument();
  expect(screen.getByText("M")).toBeVisible();
  expect(screen.queryByText("+3")).not.toBeInTheDocument();
  fireEvent.click(row);
  await waitFor(() =>
    expect(
      useWorkbenchStore
        .getState()
        .tabs.find((tab) => tab.id === useWorkbenchStore.getState().activeTabId)
        ?.kind,
    ).toBe("diff"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Open file src/app.ts" }));
  await waitFor(() =>
    expect(
      useWorkbenchStore
        .getState()
        .tabs.find((tab) => tab.id === useWorkbenchStore.getState().activeTabId)
        ?.kind,
    ).toBe("file"),
  );
});

it("preserves changes when discard is cancelled and discards only after confirmation", async () => {
  render(<SourceControlView />);
  const discard = () =>
    screen.getByRole("button", {
      name: "Discard unstaged changes to src/app.ts in Root A",
    });
  fireEvent.click(discard());
  fireEvent.click(
    screen.getByRole("button", {
      name: "Cancel discard of src/app.ts in Root A",
    }),
  );
  expect(useWorkbenchStore.getState().repositories.roots[0].changes).toEqual(
    root.changes,
  );
  fireEvent.click(discard());
  fireEvent.click(
    screen.getByRole("button", {
      name: "Confirm discard of src/app.ts in Root A",
    }),
  );
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: "Open diff for src/app.ts" }),
    ).not.toBeInTheDocument(),
  );
  expect(screen.queryByText("Staged Changes")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("0 changes")).not.toBeInTheDocument();
});

it("offers Publish Branch only for a writable named branch with a supported remote", () => {
  const { rerender } = render(<SourceControlView />);
  expect(screen.getByRole("button", { name: "Publish Branch" })).toBeEnabled();
  for (const status of [
    { ...root, remotes: [] },
    { ...root, branch: "abcdef12 (detached)" },
    { ...root, upstream: "origin/main" },
  ]) {
    useWorkbenchStore.setState({
      repositories: { roots: [status], truncated: false, elapsedMs: 1 },
    });
    rerender(<SourceControlView />);
    expect(
      screen.queryByRole("button", { name: "Publish Branch" }),
    ).not.toBeInTheDocument();
  }
});

it("offers publishing through a sole supported remote even when it is not named origin", () => {
  useWorkbenchStore.setState({
    repositories: {
      roots: [{ ...root, remotes: ["backup"] }],
      truncated: false,
      elapsedMs: 1,
    },
  });
  render(<SourceControlView />);
  expect(screen.getByRole("button", { name: "Publish Branch" })).toBeEnabled();
});

it("lets publication choose a supported remote when there is no default origin", async () => {
  useWorkbenchStore.setState({
    repositories: {
      roots: [{ ...root, remotes: ["backup", "team"] }],
      truncated: false,
      elapsedMs: 1,
    },
  });
  vi.spyOn(desktop, "pushGitRemote").mockImplementation(async (request) => {
    throw new Error(`Remote ${request.remote} rejected publication.`);
  });
  render(<SourceControlView />);
  fireEvent.click(screen.getByRole("button", { name: "Publish Branch" }));
  expect(
    screen.getAllByRole("option").map((option) => option.textContent),
  ).toEqual(["backup", "team"]);
  fireEvent.click(screen.getByRole("option", { name: "backup" }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Remote backup rejected publication.",
    ),
  );
});

it("dismisses the new-branch input with Escape and restores focus to its trigger", async () => {
  render(<StatusBar />);
  fireEvent.click(
    await screen.findByRole("button", { name: /^Switch branch/ }),
  );
  fireEvent.click(
    await screen.findByRole("option", { name: "Create new branch…" }),
  );
  const input = screen.getByLabelText("New branch name");
  input.focus();
  fireEvent.keyDown(input, { key: "Escape" });
  expect(
    screen.queryByRole("dialog", { name: "New branch in Root A" }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Switch branch in Root A" }),
  ).toHaveFocus();
});

it("creates and switches branches from the status bar through the existing commands", async () => {
  vi.spyOn(desktop, "createGitLocalBranch").mockImplementation(
    async (request, epoch) => ({
      projectId: request.projectId,
      rootId: request.rootId,
      transitionEpoch: epoch!,
      error: null,
      repositories: {
        roots: [{ ...root, branches: [...root.branches!, request.branch] }],
        truncated: false,
        elapsedMs: 1,
      },
    }),
  );
  vi.spyOn(desktop, "checkoutGitLocalBranch").mockImplementation(
    async (request, epoch) => ({
      projectId: request.projectId,
      rootId: request.rootId,
      transitionEpoch: epoch!,
      error: null,
      repositories: {
        roots: [
          {
            ...useWorkbenchStore.getState().repositories.roots[0],
            branch: request.branch,
          },
        ],
        truncated: false,
        elapsedMs: 1,
      },
    }),
  );
  render(
    <>
      <SourceControlView />
      <StatusBar />
    </>,
  );
  expect(
    screen.queryByRole("form", { name: "New branch in Root A" }),
  ).not.toBeInTheDocument();
  fireEvent.click(
    await screen.findByRole("button", { name: /^Switch branch/ }),
  );
  fireEvent.click(
    await screen.findByRole("option", { name: "Create new branch…" }),
  );
  const create = screen.getByRole("button", { name: "Create branch" });
  const input = screen.getByLabelText("New branch name");
  fireEvent.change(input, { target: { value: "-invalid" } });
  expect(create).toBeDisabled();
  fireEvent.change(input, { target: { value: "feature" } });
  fireEvent.click(create);
  await waitFor(() =>
    expect(useWorkbenchStore.getState().repositories.roots[0].branches).toEqual(
      ["main", "topic", "feature"],
    ),
  );
  expect(screen.getByText("main")).toBeVisible();
  fireEvent.click(
    await screen.findByRole("button", { name: /^Switch branch/ }),
  );
  fireEvent.click(
    await screen.findByRole("option", { name: "Switch to feature" }),
  );
  await waitFor(() =>
    expect(useWorkbenchStore.getState().repositories.roots[0].branch).toBe(
      "feature",
    ),
  );
  expect(screen.getByText("feature")).toBeVisible();
});

it("keeps a dirty branch selected and shows the checkout rejection at the status bar", async () => {
  vi.spyOn(desktop, "checkoutGitLocalBranch").mockRejectedValue(
    new Error("Commit or stash your changes before switching branches."),
  );
  render(<StatusBar />);
  fireEvent.click(
    await screen.findByRole("button", { name: /^Switch branch/ }),
  );
  fireEvent.click(
    await screen.findByRole("option", { name: "Switch to topic" }),
  );
  const dialog = await screen.findByRole("dialog", {
    name: "Cannot switch branch",
  });
  expect(within(dialog).getByRole("alert")).toHaveTextContent(
    "Commit or stash your changes before switching branches.",
  );
  expect(useWorkbenchStore.getState().repositories.roots[0].branch).toBe(
    "main",
  );
});

it("stops a smart commit when staging fails and keeps the message for recovery", async () => {
  vi.spyOn(desktop, "stageGitPaths").mockRejectedValue(
    new Error("The index is locked."),
  );
  render(<SourceControlView />);
  const message = screen.getByLabelText("Commit message for Root A");
  fireEvent.change(message, { target: { value: "Keep this message" } });
  fireEvent.click(screen.getByRole("button", { name: "Commit changes" }));
  fireEvent.click(screen.getByRole("button", { name: "Yes" }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent("The index is locked."),
  );
  expect(message).toHaveValue("Keep this message");
  expect(useWorkbenchStore.getState().repositories.roots[0].changes).toEqual(
    root.changes,
  );
  expect(
    useWorkbenchStore.getState().repositories.roots[0].recentCommits,
  ).toBeUndefined();
});

it("shows each version of a partially staged file in its group with its own status and action", () => {
  useWorkbenchStore.setState({
    repositories: {
      roots: [
        {
          ...root,
          changes: [
            {
              ...root.changes[0],
              indexStatus: "added",
              worktreeStatus: "modified",
            },
          ],
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
  });
  render(<SourceControlView />);
  const staged = screen.getByText("Staged Changes").closest("details")!;
  const changes = screen
    .getAllByRole("button", { name: "Open diff for src/app.ts" })[1]
    .closest("details")!;
  expect(within(staged).getByText("A")).toBeVisible();
  expect(within(changes).getByText("M")).toBeVisible();
  expect(
    within(staged).getByRole("button", {
      name: "Unstage src/app.ts in Root A",
    }),
  ).toBeEnabled();
  expect(
    within(staged).queryByRole("button", {
      name: "Stage src/app.ts in Root A",
    }),
  ).not.toBeInTheDocument();
  expect(
    within(changes).getByRole("button", { name: "Stage src/app.ts in Root A" }),
  ).toBeEnabled();
});

it.each(["unchanged", "modified"] as const)(
  "keeps the shared 300-row cap with %s index status when truncated",
  (indexStatus) => {
    useWorkbenchStore.setState({
      repositories: {
        roots: [
          {
            ...root,
            truncated: true,
            changes: Array.from({ length: 301 }, (_, index) => ({
              ...root.changes[0],
              indexStatus,
              resource: {
                rootId: root.rootId,
                relativePath: `src/file-${index}.ts`,
              },
            })),
          },
        ],
        truncated: true,
        elapsedMs: 1,
      },
    });
    render(<SourceControlView />);
    expect(
      screen.getAllByRole("button", { name: /^Open diff for/ }),
    ).toHaveLength(300);
    expect(screen.getByText("Change list capped at 300 files.")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Stage All Changes" }),
    ).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "Stage changes" }),
    ).not.toBeInTheDocument();
  },
);

function renderPartiallyStagedFiles() {
  useWorkbenchStore.setState({
    repositories: {
      roots: [
        {
          ...root,
          changes: Array.from({ length: 300 }, (_, index) => ({
            ...root.changes[0],
            indexStatus: "modified" as const,
            resource: {
              rootId: root.rootId,
              relativePath: `src/file-${index}.ts`,
            },
          })),
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
  });
  return render(<SourceControlView />);
}

describe("300 partially staged files", () => {
  let container: HTMLElement;
  let staged: HTMLDetailsElement;
  let changes: HTMLDetailsElement;
  beforeEach(() => {
    ({ container } = renderPartiallyStagedFiles());
    staged = screen
      .getByText("Staged Changes", { selector: "summary span" })
      .closest("details")!;
    changes = screen
      .getByText("Changes", { selector: "summary span" })
      .closest("details")!;
  });
  const expectBounded = () =>
    expect(
      container.querySelectorAll(".source-change-row").length,
    ).toBeLessThanOrEqual(300);
  it.each(["Staged Changes", "Changes"])(
    "pages all 300 files in %s with accurate group counts",
    (label) => {
      expect(within(staged).getByText("300")).toBeVisible();
      expect(within(changes).getByText("300")).toBeVisible();
      expect(container.querySelectorAll(".source-change-row")).toHaveLength(
        300,
      );
      const group = label === "Staged Changes" ? staged : changes;
      expect(group.querySelectorAll(".source-change-row")).toHaveLength(150);
      fireEvent.click(
        within(group).getByText("Show more", { selector: "button" }),
      );
      expectBounded();
      expect(
        within(group).queryByLabelText("Open diff for src/file-0.ts"),
      ).not.toBeInTheDocument();
      expect(
        within(group).getByLabelText("Open diff for src/file-299.ts"),
      ).toBeVisible();
      fireEvent.click(
        within(group).getByText("Previous", { selector: "button" }),
      );
      expectBounded();
      expect(
        within(group).getByLabelText("Open diff for src/file-0.ts"),
      ).toBeVisible();
    },
  );

  it.each(["Staged Changes", "Changes"])(
    "unmounts collapsed rows in %s and restores them on expansion",
    async (label) => {
      const group = label === "Staged Changes" ? staged : changes;
      fireEvent.click(group.querySelector("summary")!);
      await waitFor(() =>
        expect(group.querySelectorAll(".source-change-row")).toHaveLength(0),
      );
      expectBounded();
      fireEvent.click(group.querySelector("summary")!);
      await waitFor(() =>
        expect(group.querySelectorAll(".source-change-row")).toHaveLength(150),
      );
    },
  );

  describe("actions on the second page", () => {
    beforeEach(() => {
      fireEvent.click(
        within(changes).getByText("Show more", { selector: "button" }),
      );
    });
    it.each(["stage", "discard"] as const)(
      "reaches the individual %s action on the second page of 300 partially staged files",
      async (action) => {
        if (action === "stage") {
          fireEvent.click(
            within(changes).getByLabelText("Stage src/file-299.ts in Root A"),
          );
        } else {
          fireEvent.click(
            within(changes).getByLabelText(
              "Discard unstaged changes to src/file-299.ts in Root A",
            ),
          );
          expectBounded();
          fireEvent.click(
            within(changes).getByLabelText(
              "Confirm discard of src/file-299.ts in Root A",
              { selector: "button" },
            ),
          );
        }
        await waitFor(() =>
          expect(within(changes).getByText("299")).toBeVisible(),
        );
        expect(within(staged).getByText("300")).toBeVisible();
        expect(
          useWorkbenchStore.getState().repositories.roots[0].changes[299],
        ).toMatchObject({
          resource: { rootId: root.rootId, relativePath: "src/file-299.ts" },
          indexStatus: "modified",
          worktreeStatus: "unchanged",
        });
        expectBounded();
      },
    );
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
});

it("asks before staging for a commit, leaves changes on No, and stages then commits on Yes", async () => {
  render(<SourceControlView />);
  const message = screen.getByLabelText("Commit message for Root A");
  expect(message).toHaveAttribute(
    "placeholder",
    "Message (Ctrl+Enter to commit on 'main')",
  );
  fireEvent.change(message, { target: { value: "Fix app" } });
  fireEvent.click(
    screen.getByRole("button", { name: "Commit staged changes in Root A" }),
  );
  expect(screen.getByRole("dialog")).toHaveTextContent(
    "There are no staged changes to commit. Stage all your changes and commit them directly?",
  );
  fireEvent.click(screen.getByRole("button", { name: "No" }));
  expect(message).toHaveValue("Fix app");
  expect(useWorkbenchStore.getState().repositories.roots[0].changes).toEqual(
    root.changes,
  );
  fireEvent.keyDown(message, { key: "Enter", ctrlKey: true });
  fireEvent.click(screen.getByRole("button", { name: "Yes" }));
  await waitFor(() => expect(message).toHaveValue(""));
  expect(
    useWorkbenchStore.getState().repositories.roots[0].recentCommits?.[0]
      .subject,
  ).toBe("Fix app");
  expect(
    screen.queryByRole("button", { name: "Open diff for src/app.ts" }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Commit staged changes in Root A" }),
  ).toBeDisabled();
});
