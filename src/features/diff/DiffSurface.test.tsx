import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { diffCacheKey } from "../../domain/diffIdentity";
import type { WorkbenchTab } from "../../domain/workbench";
import { DiffSurface } from "./DiffSurface";

const resource = { rootId: "root-a", relativePath: "src/app.ts" };
const tab: WorkbenchTab = {
  id: "diff:src/app.ts",
  kind: "diff",
  title: "app.ts",
  position: 1,
  pinned: false,
  dirty: false,
  resource,
};
const cacheKey = diffCacheKey(undefined, resource);
const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

describe("DiffSurface resource states", () => {
  it("caps rendered lines even when an adapter returns more than the native limit", () => {
    useWorkbenchStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
      loadDiff: vi.fn(async () => undefined),
      diffs: {
        [cacheKey]: {
          phase: "ready",
          value: {
            resource,
            binary: false,
            truncated: false,
            hunks: [
              {
                header: "@@",
                lines: Array.from({ length: 1201 }, (_, index) => ({
                  kind: "added" as const,
                  oldLine: null,
                  newLine: index + 1,
                  text: "bounded line " + (index + 1),
                })),
              },
            ],
          },
        },
      },
    });
    const { container } = render(<DiffSurface tab={tab} />);
    expect(container.querySelectorAll(".diff-line")).toHaveLength(1200);
    expect(
      screen.getByText(
        "Diff capped at 1,200 lines to protect UI responsiveness.",
      ),
    ).toBeVisible();
    expect(screen.queryByText("+bounded line 1201")).toBeNull();
  });
  it("refreshes the current diff and opens its file from the review toolbar", () => {
    const loadDiff = vi.fn(async () => undefined);
    const openFile = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      diffs: {
        [cacheKey]: {
          phase: "ready",
          value: { resource, binary: false, truncated: false, hunks: [] },
        },
      },
      loadDiff,
      openFile,
    });

    render(<DiffSurface tab={tab} />);
    expect(
      screen.getByText("Changes since HEAD (or initial tree)"),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Unstaged" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Refresh diff" }));
    expect(loadDiff).toHaveBeenCalledTimes(2);
    expect(useWorkbenchStore.getState().diffs[cacheKey]).toBeUndefined();
    fireEvent.click(screen.getByRole("button", { name: "Open file" }));
    expect(openFile).toHaveBeenCalledWith(resource);
  });
  it.each([
    ["created", ["added", "added"], "+2", "-0"],
    ["deleted", ["removed", "removed"], "+0", "-2"],
  ] as const)(
    "shows %s file counts from a complete saved diff",
    (_, kinds, additions, deletions) => {
      useWorkbenchStore.setState({
        tabs: [tab],
        activeTabId: tab.id,
        repositories: { roots: [], truncated: false, elapsedMs: 0 },
        diffs: {
          [cacheKey]: {
            phase: "ready",
            value: {
              resource,
              binary: false,
              truncated: false,
              hunks: [
                {
                  header: "@@",
                  lines: kinds.map((kind, index) => ({
                    kind,
                    oldLine: kind === "removed" ? index + 1 : null,
                    newLine: kind === "added" ? index + 1 : null,
                    text: `line ${index + 1}`,
                  })),
                },
              ],
            },
          },
        },
        loadDiff: vi.fn(async () => undefined),
      });

      const { container } = render(<DiffSurface tab={tab} />);
      expect(container.querySelector(".diff-header__counts")).toHaveTextContent(
        `${additions}${deletions}`,
      );
    },
  );
  it("counts what the body shows when Git status disagrees with a complete diff", () => {
    useWorkbenchStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
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
                additions: 38,
                deletions: 14,
              },
            ],
            truncated: false,
            elapsedMs: 0,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 0,
      },
      diffs: {
        [cacheKey]: {
          phase: "ready",
          value: { resource, binary: false, truncated: false, hunks: [] },
        },
      },
      loadDiff: vi.fn(async () => undefined),
    });

    const { container } = render(<DiffSurface tab={tab} />);
    expect(screen.getByText("No textual changes for this file.")).toBeVisible();
    expect(container.querySelector(".diff-header__counts")).toHaveTextContent(
      "+0-0",
    );
  });
  it("keeps Git status counts while the diff is still loading", () => {
    useWorkbenchStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
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
                additions: 38,
                deletions: 14,
              },
            ],
            truncated: false,
            elapsedMs: 0,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 0,
      },
      diffs: { [cacheKey]: { phase: "loading" } },
      loadDiff: vi.fn(async () => undefined),
    });

    const { container } = render(<DiffSurface tab={tab} />);
    expect(container.querySelector(".diff-header__counts")).toHaveTextContent(
      "+38-14",
    );
  });
  it("moves from an announced load through an alert to a ready read-only diff", () => {
    const loadDiff = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      diffs: { [cacheKey]: { phase: "loading" } },
      loadDiff,
    });

    render(<DiffSurface tab={tab} />);

    expect(screen.getByRole("status")).toHaveTextContent("Loading diff…");
    expect(screen.getByText("Read-only")).toBeVisible();
    expect(screen.queryByText(/Explorer focused/i)).toBeNull();

    act(() => {
      useWorkbenchStore.setState({
        diffs: {
          [cacheKey]: { phase: "error", message: "Diff unavailable." },
        },
      });
    });
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Could not load this diff",
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Diff unavailable.");

    act(() => {
      useWorkbenchStore.setState({
        diffs: {
          [cacheKey]: {
            phase: "ready",
            value: {
              resource,
              binary: false,
              truncated: false,
              hunks: [
                {
                  header: "@@ -1 +1 @@",
                  lines: [
                    { kind: "removed", oldLine: 1, newLine: null, text: "old" },
                    { kind: "added", oldLine: null, newLine: 1, text: "new" },
                  ],
                },
              ],
            },
          },
        },
      });
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      screen.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    expect(loadDiff).toHaveBeenCalledOnce();
  });
});
