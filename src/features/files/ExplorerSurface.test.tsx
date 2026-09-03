import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { childResource, resourceKey } from "../../domain/resourceIdentity";
import { ExplorerSurface } from "./ExplorerSurface";

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
