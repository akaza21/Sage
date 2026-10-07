import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

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

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
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

it("indents Explorer rows from the eight pixel gutter on the sixteen pixel depth step", () => {
  const root = childResource("root-a");
  const nested = childResource("root-a", "src/first.ts");
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
              name: "first.ts",
              resource: nested,
              kind: "file",
              byteLen: 12,
            },
          ],
          truncated: false,
        },
      },
    },
    expandedDirectories: {},
  });

  render(<ExplorerSurface />);

  const gutters = screen
    .getAllByRole("treeitem")
    .map((row) => (row as HTMLElement).style.paddingLeft);
  expect(gutters.length).toBeGreaterThan(0);
  for (const gutter of gutters) {
    const px = Number.parseInt(gutter, 10);
    expect(Number.isNaN(px)).toBe(false);
    expect((px - 8) % 16).toBe(0);
  }
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
