import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { useWorkbenchStore } from "../../app/store";
import { childResource, resourceKey } from "../../domain/resourceIdentity";
import { ExplorerSurface } from "./ExplorerSurface";

const { fileKindIcon } = vi.hoisted(() => ({
  fileKindIcon: vi.fn(() => <span data-testid="file-kind-icon" />),
}));

vi.mock("./FileKindIcon", () => ({ FileKindIcon: fileKindIcon }));

Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
  configurable: true,
  value: vi.fn(),
});

const initialState = useWorkbenchStore.getState();
const initialSettings = useEditorSettingsStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  useEditorSettingsStore.setState(initialSettings, true);
});

it("keeps Add folder root on Editor Explorer, not as a chat blocker", () => {
  const addProjectRoot = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    addProjectRoot,
  });

  render(<ExplorerSurface />);
  fireEvent.click(
    screen.getByRole("button", { name: "More Explorer actions" }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: "Add folder root" }));
  expect(addProjectRoot).toHaveBeenCalledTimes(1);
});

it("asks to open a folder when Explorer has no project roots", () => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
  });

  render(<ExplorerSurface />);

  expect(screen.getByText("Open a folder to see files.")).toBeInTheDocument();
});

it("discloses the full resource path for truncated Explorer rows", () => {
  const root = childResource("root-a");
  const nested = childResource(
    "root-a",
    "src/workbench/AgentChatPanesWithALongName.tsx",
  );
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: "root-a",
          name: "A",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    selectedResource: null,
    directoryListings: {
      [resourceKey(root)]: {
        phase: "ready",
        value: {
          resource: root,
          entries: [
            {
              name: "AgentChatPanesWithALongName.tsx",
              resource: nested,
              kind: "file",
              byteLen: 120,
            },
          ],
          truncated: false,
        },
      },
    },
    expandedDirectories: {},
  });

  render(<ExplorerSurface />);

  expect(
    screen.getByRole("treeitem", {
      name: `Open file ${nested.relativePath}`,
    }),
  ).toHaveAttribute("title", nested.relativePath);
  expect(screen.getByRole("button", { name: "A" })).toHaveAttribute(
    "title",
    "A",
  );
});

it("updates only the affected memoized Explorer rows when selection changes", () => {
  const root = childResource("root-a");
  const first = childResource("root-a", "src/first.ts");
  const second = childResource("root-a", "src/second.ts");
  const unchanged = childResource("root-a", "src/unchanged.ts");
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: "root-a",
          name: "A",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    selectedResource: first,
    directoryListings: {
      [resourceKey(root)]: {
        phase: "ready",
        value: {
          resource: root,
          entries: [
            { name: "first.ts", resource: first, kind: "file", byteLen: 1 },
            { name: "second.ts", resource: second, kind: "file", byteLen: 1 },
            {
              name: "unchanged.ts",
              resource: unchanged,
              kind: "file",
              byteLen: 1,
            },
          ],
          truncated: false,
        },
      },
    },
    expandedDirectories: {},
  });

  render(<ExplorerSurface />);
  fileKindIcon.mockClear();

  act(() => useWorkbenchStore.setState({ selectedResource: second }));

  const renderedRows = (
    fileKindIcon.mock.calls as unknown as Array<[{ path?: string }]>
  ).filter(([props]) => props.path !== "A");
  expect(renderedRows).toHaveLength(2);
  expect(
    screen.getByRole("treeitem", { name: "Open file src/first.ts" }),
  ).not.toHaveAttribute("aria-current");
  expect(
    screen.getByRole("treeitem", { name: "Open file src/second.ts" }),
  ).toHaveAttribute("aria-current", "page");
});

it("dims ignored entries by default and hides them when the setting says so", () => {
  const root = childResource("root-a");
  const log = childResource("root-a", "debug.log");
  const source = childResource("root-a", "main.ts");
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: "root-a",
          name: "A",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    selectedResource: null,
    directoryListings: {
      [resourceKey(root)]: {
        phase: "ready",
        value: {
          resource: root,
          entries: [
            {
              name: "debug.log",
              resource: log,
              kind: "file",
              byteLen: 3,
              ignored: true,
            },
            { name: "main.ts", resource: source, kind: "file", byteLen: 3 },
          ],
          truncated: false,
        },
      },
    },
    expandedDirectories: {},
  });

  render(<ExplorerSurface />);

  const ignoredRow = screen.getByRole("treeitem", {
    name: "Open file debug.log",
  });
  expect(ignoredRow).toHaveClass("tree__row--ignored");
  expect(
    screen.getByRole("treeitem", { name: "Open file main.ts" }),
  ).not.toHaveClass("tree__row--ignored");

  act(() => {
    useEditorSettingsStore.setState({
      settings: {
        ...useEditorSettingsStore.getState().settings,
        "explorer.showIgnored": "hidden",
      },
    });
  });
  expect(
    screen.queryByRole("treeitem", { name: "Open file debug.log" }),
  ).toBeNull();
  expect(
    screen.getByRole("treeitem", { name: "Open file main.ts" }),
  ).toBeInTheDocument();
});

it("hides what files.exclude names, .DS_Store by default", async () => {
  const root = childResource("root-a");
  const entry = (name: string) => ({
    name,
    resource: childResource("root-a", name),
    kind: "file" as const,
    byteLen: 3,
  });
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: "root-a",
          name: "A",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    selectedResource: null,
    directoryListings: {
      [resourceKey(root)]: {
        phase: "ready",
        value: {
          resource: root,
          entries: [entry(".DS_Store"), entry("main.ts"), entry("notes.tmp")],
          truncated: false,
        },
      },
    },
    expandedDirectories: {},
  });

  render(<ExplorerSurface />);

  const shown = () =>
    screen
      .getAllByRole("treeitem")
      .map((row) => row.getAttribute("aria-label"))
      .filter((label) => label?.startsWith("Open file"));
  // The matcher loads on its own; the default list then hides .DS_Store.
  await waitFor(() =>
    expect(shown()).toEqual(["Open file main.ts", "Open file notes.tmp"]),
  );

  act(() => {
    useEditorSettingsStore.setState({
      settings: {
        ...useEditorSettingsStore.getState().settings,
        "files.exclude": ["**/*.tmp"],
      },
    });
  });
  expect(shown()).toEqual(["Open file .DS_Store", "Open file main.ts"]);
});

it("marks changed Files and the folders holding them from the Git status (EP-13)", async () => {
  const root = childResource("root-a");
  const src = childResource("root-a", "src");
  const edited = childResource("root-a", "src/edited.ts");
  const fresh = childResource("root-a", "notes.md");
  const log = childResource("root-a", "build.log");
  const clean = childResource("root-a", "clean.ts");
  const status = (
    relativePath: string,
    worktreeStatus: "modified" | "untracked",
  ) => ({
    resource: { rootId: "root-a", relativePath },
    indexStatus: "unchanged" as const,
    worktreeStatus,
    additions: null,
    deletions: null,
  });
  const repository = {
    rootId: "root-a",
    state: "repository" as const,
    branch: "main",
    changes: [
      status("src/edited.ts", "modified"),
      status("notes.md", "untracked"),
      status("build.log", "modified"),
    ],
    truncated: false,
    elapsedMs: 3,
    message: null,
  };
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: "root-a",
          name: "A",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    repositories: { roots: [repository], truncated: false, elapsedMs: 3 },
    selectedResource: null,
    directoryListings: {
      [resourceKey(root)]: {
        phase: "ready",
        value: {
          resource: root,
          entries: [
            { name: "src", resource: src, kind: "directory", byteLen: null },
            { name: "notes.md", resource: fresh, kind: "file", byteLen: 1 },
            {
              name: "build.log",
              resource: log,
              kind: "file",
              byteLen: 1,
              ignored: true,
            },
            { name: "clean.ts", resource: clean, kind: "file", byteLen: 1 },
          ],
          truncated: false,
        },
      },
      [resourceKey(src)]: {
        phase: "ready",
        value: {
          resource: src,
          entries: [
            { name: "edited.ts", resource: edited, kind: "file", byteLen: 1 },
          ],
          truncated: false,
        },
      },
    },
    expandedDirectories: { [resourceKey(src)]: true },
  });

  render(<ExplorerSurface />);

  // The marks arrive with their lazy chunk.
  const folder = await screen.findByRole("treeitem", {
    name: "Toggle folder src, Contains changes",
  });
  expect(folder).toHaveClass("tree__row--git", "git-tone--modified");
  expect(folder).toHaveTextContent("src•");
  const editedRow = screen.getByRole("treeitem", {
    name: "Open file src/edited.ts, Modified",
  });
  expect(editedRow).toHaveTextContent("edited.tsM");
  expect(editedRow).toHaveAttribute("title", "src/edited.ts, Modified");
  expect(
    screen.getByRole("treeitem", { name: "Open file notes.md, Untracked" }),
  ).toHaveClass("git-tone--untracked");
  // Ignored and changed: both states show.
  expect(
    screen.getByRole("treeitem", { name: "Open file build.log, Modified" }),
  ).toHaveClass("tree__row--ignored", "tree__row--git");
  expect(
    screen.getByRole("treeitem", { name: "Open file clean.ts" }),
  ).not.toHaveClass("tree__row--git");
  expect(
    screen.getByRole("button", { name: "A, Contains changes" }),
  ).toHaveClass("root-tree__heading--git");

  // An EP-12 refresh that changes the status updates the marks.
  act(() => {
    useWorkbenchStore.setState({
      repositories: {
        roots: [{ ...repository, changes: [status("clean.ts", "modified")] }],
        truncated: false,
        elapsedMs: 3,
      },
    });
  });
  expect(
    screen.getByRole("treeitem", { name: "Toggle folder src" }),
  ).not.toHaveClass("tree__row--git");
  expect(
    screen.getByRole("treeitem", { name: "Open file clean.ts, Modified" }),
  ).toBeInTheDocument();

  // `git.decorations` off: no marks.
  act(() => {
    useEditorSettingsStore.setState({
      settings: {
        ...useEditorSettingsStore.getState().settings,
        "git.decorations": false,
      },
    });
  });
  expect(
    screen.getByRole("treeitem", { name: "Open file clean.ts" }),
  ).not.toHaveClass("tree__row--git");
  expect(screen.getByRole("button", { name: "A" })).not.toHaveClass(
    "root-tree__heading--git",
  );
});

it("keeps unchanged Explorer rows from re-rendering when a refresh changes another File", async () => {
  const root = childResource("root-a");
  const a = childResource("root-a", "a.ts");
  const change = (relativePath: string) => ({
    resource: { rootId: "root-a", relativePath },
    indexStatus: "unchanged" as const,
    worktreeStatus: "modified" as const,
    additions: null,
    deletions: null,
  });
  const repo = (paths: string[]) => ({
    roots: [
      {
        rootId: "root-a",
        state: "repository" as const,
        branch: "main",
        changes: paths.map(change),
        truncated: false,
        elapsedMs: 1,
        message: null,
      },
    ],
    truncated: false,
    elapsedMs: 1,
  });
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: "root-a",
          name: "A",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    repositories: repo(["a.ts"]),
    selectedResource: null,
    directoryListings: {
      [resourceKey(root)]: {
        phase: "ready",
        value: {
          resource: root,
          entries: [{ name: "a.ts", resource: a, kind: "file", byteLen: 1 }],
          truncated: false,
        },
      },
    },
    expandedDirectories: {},
  });
  render(<ExplorerSurface />);
  await screen.findByRole("treeitem", { name: "Open file a.ts, Modified" });
  const iconRenders = fileKindIcon.mock.calls.length;
  act(() => {
    useWorkbenchStore.setState({ repositories: repo(["a.ts", "b.ts"]) });
  });
  // The heading re-renders; the a.ts row (memoized) does not.
  expect(fileKindIcon.mock.calls.length - iconRenders).toBe(1);
});

function expandedRoots(rootIds: string[]) {
  const roots = rootIds.map((id, ordinal) => ({
    id,
    name: id,
    ordinal,
    availability: "available" as const,
    message: null,
  }));
  const directoryListings: ReturnType<
    typeof useWorkbenchStore.getState
  >["directoryListings"] = {};
  const expandedDirectories: Record<string, boolean> = {};
  for (const root of roots) {
    const resource = childResource(root.id);
    const folder = childResource(root.id, "src");
    directoryListings[resourceKey(resource)] = {
      phase: "ready",
      value: {
        resource,
        truncated: false,
        entries: [
          { name: "src", resource: folder, kind: "directory", byteLen: null },
        ],
      },
    };
    directoryListings[resourceKey(folder)] = {
      phase: "ready",
      value: {
        resource: folder,
        truncated: false,
        entries: Array.from({ length: 399 }, (_, index) => ({
          name: `file-${index}.ts`,
          resource: childResource(root.id, `src/file-${index}.ts`),
          kind: "file" as const,
          byteLen: 1,
        })),
      },
    };
    expandedDirectories[resourceKey(folder)] = true;
  }
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots },
    directoryListings,
    expandedDirectories,
    selectedResource: null,
  });
}

it("shares the view cap across expanded roots and reallocates it after collapse", () => {
  expandedRoots(["root-a", "root-b"]);
  render(<ExplorerSurface />);
  const rows = screen.getAllByRole("treeitem");
  expect(rows).toHaveLength(500);
  expect(rows[399]).toHaveAttribute(
    "data-resource-key",
    resourceKey(childResource("root-a", "src/file-398.ts")),
  );
  expect(rows[499]).toHaveAttribute(
    "data-resource-key",
    resourceKey(childResource("root-b", "src/file-98.ts")),
  );
  expect(
    screen.getByText(/More entries are hidden by the view limit/),
  ).toBeInTheDocument();

  act(() =>
    useWorkbenchStore.setState({
      expandedDirectories: {
        [resourceKey(childResource("root-b", "src"))]: true,
      },
    }),
  );
  expect(screen.getAllByRole("treeitem")).toHaveLength(401);
  expect(
    screen.getByRole("treeitem", { name: "Open file src/file-398.ts" }),
  ).toHaveAttribute(
    "data-resource-key",
    resourceKey(childResource("root-b", "src/file-398.ts")),
  );
  expect(
    screen.queryByText(/More entries are hidden by the view limit/),
  ).not.toBeInTheDocument();
});

it("renders no entries from later roots once the shared budget is exhausted", () => {
  expandedRoots(["root-a", "root-b", "root-c"]);
  render(<ExplorerSurface />);
  expect(screen.getAllByRole("treeitem")).toHaveLength(500);
  expect(
    screen
      .getAllByRole("treeitem")
      .some(
        (row) =>
          row.dataset.resourceKey ===
          resourceKey(childResource("root-c", "src")),
      ),
  ).toBe(false);
  expect(
    screen.getAllByText(/More entries are hidden by the view limit/),
  ).toHaveLength(2);
});

it("preserves all rows and their order for a single root below the cap", () => {
  expandedRoots(["root-a"]);
  render(<ExplorerSurface />);
  expect(
    screen.getAllByRole("treeitem").map((row) => row.dataset.resourceKey),
  ).toEqual([
    resourceKey(childResource("root-a", "src")),
    ...Array.from({ length: 399 }, (_, index) =>
      resourceKey(childResource("root-a", `src/file-${index}.ts`)),
    ),
  ]);
  expect(
    screen.queryByText(/More entries are hidden by the view limit/),
  ).not.toBeInTheDocument();
});
