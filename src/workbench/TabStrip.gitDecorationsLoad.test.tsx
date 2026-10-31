import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { GitFileState } from "../domain/repository";
import type { WorkbenchTab } from "../domain/workbench";

// Holds the decoration chunk's import until a test releases it, as a slow
// cold load would.
const chunk = {
  state: { imports: 0, release: () => {}, gate: Promise.resolve() },
  hold() {
    this.state.gate = new Promise<void>((resolve) => {
      this.state.release = resolve;
    });
  },
};

beforeEach(() => {
  // A fresh module graph each test, so every test starts with the chunk cold.
  vi.resetModules();
  vi.doMock("../features/git/gitDecorationsUi", async (importOriginal) => {
    chunk.state.imports += 1;
    await chunk.state.gate;
    return importOriginal();
  });
  chunk.state.imports = 0;
  chunk.hold();
});

afterEach(() => {
  chunk.state.release();
  cleanup();
});

const tab = (id: string, relativePath: string): WorkbenchTab => ({
  id,
  kind: "file",
  title: relativePath,
  position: 0,
  pinned: false,
  dirty: false,
  resource: { rootId: "root-a", relativePath },
});

const repositories = (
  state: "repository" | "notRepository",
  changes: [string, GitFileState][],
) => ({
  roots: [
    {
      rootId: "root-a",
      state,
      branch: state === "repository" ? "main" : null,
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

async function mountTabs(options: {
  decorations: boolean;
  repository: ReturnType<typeof repositories>;
}) {
  const [{ useWorkbenchStore }, { useEditorSettingsStore }, { TabStrip }] =
    await Promise.all([
      import("../app/store"),
      import("../app/editorSettingsStore"),
      import("./TabStrip"),
    ]);
  const setDecorations = (on: boolean) =>
    act(() => {
      useEditorSettingsStore.setState({
        settings: {
          ...useEditorSettingsStore.getState().settings,
          "git.decorations": on,
        },
      });
    });
  setDecorations(options.decorations);
  useWorkbenchStore.setState({
    tabs: [tab("file:a", "a.ts"), tab("file:b", "b.ts")],
    activeTabId: "file:a",
    repositories: options.repository,
  });
  render(<TabStrip showNewChat={false} />);
  return {
    setDecorations,
    setRepository: (repository: ReturnType<typeof repositories>) =>
      act(() => {
        useWorkbenchStore.setState({ repositories: repository });
      }),
  };
}

const changed = repositories("repository", [["a.ts", "modified"]]);
const tabA = () => screen.getByRole("tab", { name: /^a\.ts/ });
const tabB = () => screen.getByRole("tab", { name: /^b\.ts/ });
const releaseChunk = () =>
  act(async () => {
    chunk.state.release();
    await chunk.state.gate;
  });

it("holds a changed tab's mark room while the decorations load, so the strip does not shift (EP-13)", async () => {
  await mountTabs({ decorations: true, repository: changed });

  // Loading: the changed tab already holds the mark's room; the clean one
  // does not, and neither shows a mark yet.
  expect(tabA().querySelector(".tab__git-slot")).not.toBeNull();
  expect(tabB().querySelector(".tab__git-slot")).toBeNull();
  expect(document.querySelector(".git-badge")).toBeNull();
  expect(tabA().lastElementChild).toHaveClass("tab__git-slot");

  await releaseChunk();

  // Loaded: the mark takes the room it held, in the same place.
  const edited = await screen.findByRole("tab", { name: "a.ts, Modified" });
  expect(edited.querySelector(".tab__git-slot")).toBeNull();
  expect(edited.lastElementChild).toHaveClass("git-badge");
  expect(edited.lastElementChild).toHaveTextContent("M");
  expect(tabB().querySelector(".git-badge, .tab__git-slot")).toBeNull();
  expect(chunk.state.imports).toBe(1);
});

it("loads nothing while decorations are off or outside Git, then marks tabs once both allow it", async () => {
  const { setDecorations, setRepository } = await mountTabs({
    decorations: false,
    repository: repositories("notRepository", []),
  });
  expect(document.querySelector(".git-badge, .tab__git-slot")).toBeNull();

  setDecorations(true);
  expect(document.querySelector(".git-badge, .tab__git-slot")).toBeNull();
  await releaseChunk();
  expect(chunk.state.imports).toBe(0);

  setRepository(changed);
  expect(
    await screen.findByRole("tab", { name: "a.ts, Modified" }),
  ).toBeInTheDocument();
  expect(chunk.state.imports).toBe(1);
});

it("loads nothing while decorations start off inside a Git repository, then marks tabs when turned on", async () => {
  const { setDecorations } = await mountTabs({
    decorations: false,
    repository: changed,
  });
  expect(document.querySelector(".git-badge, .tab__git-slot")).toBeNull();
  await releaseChunk();
  expect(chunk.state.imports).toBe(0);
  expect(tabA()).toHaveAccessibleName("a.ts");

  setDecorations(true);
  expect(
    await screen.findByRole("tab", { name: "a.ts, Modified" }),
  ).toBeInTheDocument();
  expect(chunk.state.imports).toBe(1);
});

it("shows no marks or held room when decorations are turned off while they load", async () => {
  const { setDecorations } = await mountTabs({
    decorations: true,
    repository: changed,
  });
  expect(tabA().querySelector(".tab__git-slot")).not.toBeNull();

  setDecorations(false);
  expect(document.querySelector(".git-badge, .tab__git-slot")).toBeNull();

  await releaseChunk();
  expect(document.querySelector(".git-badge, .tab__git-slot")).toBeNull();
  expect(tabA()).toHaveAccessibleName("a.ts");

  // Back on: the marks come back.
  setDecorations(true);
  expect(
    await screen.findByRole("tab", { name: "a.ts, Modified" }),
  ).toBeInTheDocument();
});
