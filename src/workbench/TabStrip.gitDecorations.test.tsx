import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { useWorkbenchStore } from "../app/store";
import type { GitFileState } from "../domain/repository";
import type { WorkbenchTab } from "../domain/workbench";
import { TabStrip } from "./TabStrip";

const initialState = useWorkbenchStore.getState();
const initialSettings = useEditorSettingsStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  useEditorSettingsStore.setState(initialSettings, true);
});

const tab = (
  id: string,
  kind: WorkbenchTab["kind"],
  relativePath: string,
): WorkbenchTab => ({
  id,
  kind,
  title: relativePath.split("/").pop() ?? relativePath,
  position: 0,
  pinned: false,
  dirty: false,
  resource: { rootId: "root-a", relativePath },
});

const repositories = (changes: [string, GitFileState][]) => ({
  roots: [
    {
      rootId: "root-a",
      state: "repository" as const,
      branch: "main",
      changes: changes.map(([relativePath, worktreeStatus]) => ({
        resource: { rootId: "root-a", relativePath },
        indexStatus: "unchanged" as const,
        worktreeStatus,
        additions: null,
        deletions: null,
      })),
      truncated: false,
      elapsedMs: 1,
      message: null,
    },
  ],
  truncated: false,
  elapsedMs: 1,
});

it("marks File tabs with their Git status and follows refreshes (EP-13)", async () => {
  useWorkbenchStore.setState({
    tabs: [
      tab("file:a", "file", "src/a.ts"),
      tab("file:b", "file", "src/b.ts"),
      tab("diff:a", "diff", "src/a.ts"),
    ],
    activeTabId: "file:a",
    repositories: repositories([
      ["src/a.ts", "modified"],
      ["src/b.ts", "deleted"],
    ]),
  });

  render(<TabStrip showNewChat={false} />);

  const edited = await screen.findByRole("tab", { name: "a.ts, Modified" });
  expect(edited).toHaveAttribute("title", "src/a.ts");
  expect(edited.querySelector(".git-badge")).toHaveTextContent("M");
  expect(
    screen
      .getByRole("tab", { name: "b.ts, Deleted" })
      .querySelector(".tab__title--git"),
  ).toHaveClass("git-tone--deleted");
  // A Diff tab already shows the change.
  expect(screen.getByRole("tab", { name: "a.ts" })).toHaveAttribute(
    "title",
    "src/a.ts",
  );

  await act(async () => {
    useWorkbenchStore.setState({
      repositories: repositories([["src/b.ts", "untracked"]]),
    });
  });
  expect(screen.getAllByRole("tab", { name: "a.ts" })).toHaveLength(2);
  expect(
    await screen.findByRole("tab", { name: "b.ts, Untracked" }),
  ).toBeTruthy();

  await act(async () => {
    useEditorSettingsStore.setState({
      settings: {
        ...useEditorSettingsStore.getState().settings,
        "git.decorations": false,
      },
    });
  });
  expect(screen.getByRole("tab", { name: "b.ts" })).toBeInTheDocument();
  expect(document.querySelector(".git-badge")).toBeNull();
});
