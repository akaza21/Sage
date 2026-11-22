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
import { useWorkbenchStore } from "../../app/store";
import type { RootRepositoryStatus } from "../../domain/workbench";
import { desktop } from "../../services/desktop";
import { ComposerLocationStrip } from "./ComposerLocationStrip";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(useWorkbenchStore.getInitialState(), true);
});

it("offers checked local work and announces remote work as unavailable", async () => {
  render(
    <ComposerLocationStrip projectName="Choose project" disabled={false} />,
  );
  const trigger = screen.getByRole("button", { name: "Work in" });
  fireEvent.click(trigger);
  const menu = await screen.findByRole("listbox", { name: "Work in" });
  expect(
    within(menu).getByRole("option", { name: "This computer" }),
  ).toHaveAttribute("aria-selected", "true");
  const remote = within(menu).getByRole("option", { name: /Remote/ });
  expect(remote).toBeDisabled();
  expect(remote).toHaveAttribute("aria-disabled", "true");
  expect(remote).toHaveAccessibleDescription("Not set up yet");
  fireEvent.click(remote);
  expect(menu).toBeVisible();
  fireEvent.click(within(menu).getByRole("option", { name: "This computer" }));
  expect(screen.queryByRole("listbox", { name: "Work in" })).toBeNull();
  expect(trigger).toHaveFocus();
  expect(trigger).toHaveTextContent("This computer");
});

it("hides outside Git, locks during a turn, and switches through shared dirty checkout", async () => {
  const repository: RootRepositoryStatus = {
    rootId: "root-a",
    state: "notRepository",
    branch: null,
    branches: ["main", "topic"],
    changes: [],
    truncated: false,
    elapsedMs: 1,
    message: null,
  };
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "Alpha",
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
    repositories: { roots: [repository], truncated: false, elapsedMs: 1 },
  });
  const view = render(
    <ComposerLocationStrip projectName="Alpha" disabled={false} />,
  );
  await screen.findByRole("button", { name: "Project: Alpha" });
  expect(screen.queryByRole("button", { name: /Switch branch/ })).toBeNull();
  act(() =>
    useWorkbenchStore.setState({
      repositories: {
        roots: [{ ...repository, state: "repository", branch: "main" }],
        truncated: false,
        elapsedMs: 1,
      },
    }),
  );
  const chip = await screen.findByRole("button", {
    name: "Switch branch in Alpha",
  });
  expect(chip).toHaveTextContent("main");
  view.rerender(
    <ComposerLocationStrip projectName="Alpha" disabled={false} turnBusy />,
  );
  expect(chip).toBeDisabled();
  expect(chip).toHaveAttribute(
    "title",
    "Can't switch branches while the agent is working",
  );
  fireEvent.click(chip);
  expect(screen.queryByRole("listbox")).toBeNull();
  view.rerender(<ComposerLocationStrip projectName="Alpha" disabled={false} />);
  act(() =>
    useWorkbenchStore.setState((state) => ({
      repositories: {
        ...state.repositories,
        roots: [
          {
            ...state.repositories.roots[0],
            changes: [
              {
                resource: { rootId: "root-a", relativePath: "app.ts" },
                indexStatus: "unchanged",
                worktreeStatus: "modified",
                additions: 1,
                deletions: 0,
              },
            ],
          },
        ],
      },
    })),
  );
  vi.spyOn(desktop, "checkoutGitLocalBranch")
    .mockRejectedValueOnce(
      new Error("Commit or stash your changes before switching branches."),
    )
    .mockImplementation(async (request, epoch) => ({
      projectId: request.projectId,
      rootId: request.rootId,
      transitionEpoch: epoch!,
      error: null,
      repositories: {
        ...useWorkbenchStore.getState().repositories,
        roots: [
          {
            ...useWorkbenchStore.getState().repositories.roots[0],
            branch: request.branch,
          },
        ],
      },
    }));
  fireEvent.keyDown(chip, { key: "ArrowDown" });
  const menu = await screen.findByRole("listbox", {
    name: "Switch branch in Alpha",
  });
  expect(
    within(menu).queryByRole("option", { name: "Rename branch…" }),
  ).toBeNull();
  expect(
    within(menu).queryByRole("option", { name: "Delete branch…" }),
  ).toBeNull();
  expect(
    within(menu).getByRole("option", { name: "Create new branch…" }),
  ).toBeEnabled();
  fireEvent.change(screen.getByPlaceholderText("Select a branch"), {
    target: { value: "topic" },
  });
  fireEvent.click(
    within(menu).getByRole("option", { name: "Switch to topic" }),
  );
  const confirm = await screen.findByRole("dialog", {
    name: "Switch branch with uncommitted changes?",
  });
  fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
  expect(
    screen.getByRole("button", { name: "Switch branch in Alpha" }),
  ).toHaveTextContent("main");
  fireEvent.click(
    screen.getByRole("button", { name: "Switch branch in Alpha" }),
  );
  fireEvent.click(
    await screen.findByRole("option", { name: "Switch to topic" }),
  );
  fireEvent.click(
    within(
      await screen.findByRole("dialog", {
        name: "Switch branch with uncommitted changes?",
      }),
    ).getByRole("button", { name: "Switch branch" }),
  );
  const error = await screen.findByRole("dialog", {
    name: "Cannot switch branch",
  });
  expect(within(error).getByRole("alert")).toHaveTextContent(
    "Commit or stash your changes before switching branches.",
  );
  expect(useWorkbenchStore.getState().repositories.roots[0].branch).toBe(
    "main",
  );
  fireEvent.click(within(error).getByRole("button", { name: "Close" }));
  fireEvent.click(
    screen.getByRole("button", { name: "Switch branch in Alpha" }),
  );
  fireEvent.click(
    await screen.findByRole("option", { name: "Switch to topic" }),
  );
  fireEvent.click(
    within(
      await screen.findByRole("dialog", {
        name: "Switch branch with uncommitted changes?",
      }),
    ).getByRole("button", { name: "Switch branch" }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Switch branch in Alpha" }),
    ).toHaveTextContent("topic"),
  );
  expect(useWorkbenchStore.getState().repositories.roots[0].branch).toBe(
    "topic",
  );
  view.rerender(
    <ComposerLocationStrip projectName="Alpha" disabled={false} turnBusy />,
  );
  expect(
    screen.getByRole("button", { name: "Switch branch in Alpha" }),
  ).toBeDisabled();
  expect(
    screen.getByRole("button", { name: "Switch branch in Alpha" }),
  ).toHaveAttribute(
    "title",
    "Can't switch branches while the agent is working",
  );
  act(() =>
    useWorkbenchStore.setState({
      repositories: { roots: [repository], truncated: false, elapsedMs: 1 },
    }),
  );
  expect(screen.queryByRole("button", { name: /Switch branch/ })).toBeNull();
});
