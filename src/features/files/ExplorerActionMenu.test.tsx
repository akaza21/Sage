import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { childResource, resourceKey } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import { ExplorerReleasesOpenFiles } from "./explorerOpenFiles";
import { ExplorerSurface } from "./ExplorerSurface";

const initialState = useWorkbenchStore.getState();
const root = childResource("root-a");

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

function installSelection({
  selected,
  kind,
  children = [],
  parentTruncated = false,
  open = false,
  addProjectRoot = vi.fn(async () => undefined),
}: {
  selected: ResourceRef | null;
  kind?: "file" | "directory" | "symlink";
  children?: ResourceRef[];
  parentTruncated?: boolean;
  open?: boolean;
  addProjectRoot?: () => Promise<void>;
}) {
  const parent = selected
    ? childResource(
        selected.rootId,
        selected.relativePath.split("/").slice(0, -1).join("/"),
      )
    : root;
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
    projectGeneration: 3,
    nativeTransitionEpoch: 4,
    projectTransitioning: false,
    selectedRootId: "root-a",
    selectedResource: selected,
    tabs:
      open && selected
        ? [
            {
              id: "open-resource",
              kind: "file",
              title: selected.relativePath,
              position: 0,
              pinned: false,
              dirty: false,
              resource: selected,
            },
          ]
        : [],
    activeTabId: open ? "open-resource" : null,
    activeEditor: null,
    directoryListings:
      selected && kind
        ? {
            [resourceKey(parent)]: {
              phase: "ready",
              value: {
                resource: parent,
                entries: [
                  {
                    name: selected.relativePath.split("/").at(-1) ?? "",
                    resource: selected,
                    kind,
                    byteLen: kind === "file" ? 8 : null,
                  },
                ],
                truncated: parentTruncated,
              },
            },
            ...(kind === "directory"
              ? {
                  [resourceKey(selected)]: {
                    phase: "ready" as const,
                    value: {
                      resource: selected,
                      entries: children.map((child) => ({
                        name: child.relativePath.split("/").at(-1) ?? "",
                        resource: child,
                        kind: "file" as const,
                        byteLen: 8,
                      })),
                      truncated: false,
                    },
                  },
                }
              : {}),
          }
        : {},
    addProjectRoot,
  });
  return addProjectRoot;
}

function openMenu() {
  const more = screen.getByRole("button", { name: "More Explorer actions" });
  fireEvent.click(more);
  return {
    more,
    menu: screen.getByRole("menu", { name: "Explorer actions" }),
  };
}

describe("ExplorerActionMenu", () => {
  it("keeps creation direct and gives the compact menu complete keyboard focus", async () => {
    const source = childResource("root-a", "src/closed.ts");
    const addProjectRoot = installSelection({ selected: source, kind: "file" });
    render(<ExplorerSurface />);

    expect(screen.getByRole("button", { name: "New File" })).toBeVisible();
    expect(screen.getByRole("button", { name: "New Folder" })).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Add folder root" }),
    ).not.toBeInTheDocument();

    const { menu, more } = openMenu();
    const items = within(menu).getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "Add folder root",
      "Rename File",
      "Move to Trash",
    ]);
    expect(items[0]).toHaveFocus();

    fireEvent.keyDown(menu, { key: "End" });
    expect(items[2]).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(items[0]).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(items[2]).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Home" });
    expect(items[0]).toHaveFocus();

    fireEvent.click(items[0]);
    expect(addProjectRoot).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    await vi.waitFor(() => expect(more).toHaveFocus());

    fireEvent.click(more);
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    await vi.waitFor(() => expect(more).toHaveFocus());

    fireEvent.click(more);
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    await vi.waitFor(() => expect(more).toHaveFocus());
  });

  it("opens toward the requested edge, roves one tab stop, and dismisses on Tab", () => {
    const source = childResource("root-a", "src/closed.ts");
    installSelection({ selected: source, kind: "file" });
    render(<ExplorerSurface />);

    const more = screen.getByRole("button", { name: "More Explorer actions" });
    more.focus();
    fireEvent.keyDown(more, { key: "ArrowDown" });

    let menu = screen.getByRole("menu", { name: "Explorer actions" });
    let items = within(menu).getAllByRole("menuitem");
    expect(items[0]).toHaveFocus();
    expect(items.map((item) => item.tabIndex)).toEqual([0, -1, -1]);

    fireEvent.keyDown(menu, { key: "End" });
    expect(items[2]).toHaveFocus();
    expect(items.map((item) => item.tabIndex)).toEqual([-1, -1, 0]);
    fireEvent.keyDown(menu, { key: "Tab" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(more).toHaveAttribute("aria-expanded", "false");

    fireEvent.keyDown(more, { key: "ArrowUp" });
    menu = screen.getByRole("menu", { name: "Explorer actions" });
    items = within(menu).getAllByRole("menuitem");
    expect(items[2]).toHaveFocus();
    expect(items.map((item) => item.tabIndex)).toEqual([-1, -1, 0]);
    fireEvent.keyDown(menu, { key: "Tab", shiftKey: true });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(more).toHaveAttribute("aria-expanded", "false");
  });

  it("includes a disabled reason in arrow navigation without activating it", async () => {
    const addProjectRoot = installSelection({
      selected: null,
      kind: undefined,
    });
    render(<ExplorerSurface />);

    const more = screen.getByRole("button", { name: "More Explorer actions" });
    more.focus();
    fireEvent.keyDown(more, { key: "ArrowDown" });

    let menu = screen.getByRole("menu", { name: "Explorer actions" });
    let items = within(menu).getAllByRole("menuitem");
    const reason = within(menu).getByRole("menuitem", {
      name: "Select a visible closed file or empty folder.",
    });
    expect(items[0]).toHaveFocus();
    expect(items.map((item) => item.tabIndex)).toEqual([0, -1]);

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(reason).toHaveFocus();
    expect(items.map((item) => item.tabIndex)).toEqual([-1, 0]);
    fireEvent.keyDown(reason, { key: "Enter" });
    expect(addProjectRoot).not.toHaveBeenCalled();
    expect(menu).toBeVisible();

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(items[0]).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(reason).toHaveFocus();

    fireEvent.keyDown(menu, { key: "Escape" });
    await vi.waitFor(() => expect(more).toHaveFocus());
    fireEvent.keyDown(more, { key: "ArrowUp" });
    menu = screen.getByRole("menu", { name: "Explorer actions" });
    items = within(menu).getAllByRole("menuitem");
    expect(reason).toHaveFocus();
    expect(items.map((item) => item.tabIndex)).toEqual([-1, 0]);
  });

  it.each([
    {
      name: "an open file",
      selected: childResource("root-a", "src/open.ts"),
      kind: "file" as const,
      open: true,
      parentTruncated: false,
      children: [],
      reason: "Close this resource before renaming or recycling it.",
    },
    {
      name: "a non-empty folder",
      selected: childResource("root-a", "src/filled"),
      kind: "directory" as const,
      open: false,
      parentTruncated: false,
      children: [childResource("root-a", "src/filled/child.ts")],
      reason: "Only empty folders can be renamed or recycled.",
    },
    {
      name: "a selection from a truncated listing",
      selected: childResource("root-a", "src/uncertain.ts"),
      kind: "file" as const,
      open: false,
      parentTruncated: true,
      children: [],
      reason: "Explorer cannot act on a selection from an incomplete listing.",
    },
    {
      name: "no supported selection",
      selected: null,
      kind: undefined,
      open: false,
      parentTruncated: false,
      children: [],
      reason: "Select a visible closed file or empty folder.",
    },
  ])("shows one plain reason for $name", (selection) => {
    installSelection(selection);
    render(<ExplorerSurface />);

    const { menu } = openMenu();
    const items = within(menu).getAllByRole("menuitem");
    expect(items).toHaveLength(2);
    const reason = within(menu).getByRole("menuitem", {
      name: selection.reason,
    });
    expect(reason).toHaveClass("explorer-action-menu__reason");
    expect(reason).toHaveAttribute("aria-disabled", "true");
    expect(reason).toHaveAttribute("tabindex", "-1");
    expect(
      menu.querySelectorAll('[role="menuitem"]:not([aria-disabled="true"])'),
    ).toHaveLength(1);
    expect(
      within(menu).queryByRole("menuitem", { name: /Rename/ }),
    ).not.toBeInTheDocument();
    expect(
      within(menu).queryByRole("menuitem", { name: "Move to Trash" }),
    ).not.toBeInTheDocument();
  });

  it("opens the existing rename dialog and returns focus to More when it closes", async () => {
    const source = childResource("root-a", "src/closed.ts");
    installSelection({ selected: source, kind: "file" });
    render(<ExplorerSurface />);

    const { menu, more } = openMenu();
    fireEvent.click(
      within(menu).getByRole("menuitem", { name: "Rename File" }),
    );

    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(await screen.findByLabelText("File name")).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await vi.waitFor(() => expect(more).toHaveFocus());
  });

  it("renames and recycles an open File in the Editor, releasing its tab", async () => {
    const source = childResource("root-a", "src/open.ts");
    installSelection({ selected: source, kind: "file", open: true });
    const renameProjectFile = vi.fn(async () => true);
    const trashProjectResource = vi.fn(async () => true);
    useWorkbenchStore.setState({ renameProjectFile, trashProjectResource });
    render(
      <ExplorerReleasesOpenFiles.Provider value={true}>
        <ExplorerSurface />
      </ExplorerReleasesOpenFiles.Provider>,
    );

    let { menu } = openMenu();
    fireEvent.click(
      within(menu).getByRole("menuitem", { name: "Rename File" }),
    );
    fireEvent.change(await screen.findByLabelText("File name"), {
      target: { value: "renamed.ts" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    await vi.waitFor(() =>
      expect(renameProjectFile).toHaveBeenCalledWith(
        source,
        "renamed.ts",
        true,
      ),
    );

    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    ({ menu } = openMenu());
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Move to/u }));
    fireEvent.click(await screen.findByRole("button", { name: /Move to/u }));
    await vi.waitFor(() =>
      expect(trashProjectResource).toHaveBeenCalledWith(source, "file", true),
    );
  });

  it("explains that an open File with unsaved changes must be saved first", () => {
    const source = childResource("root-a", "src/open.ts");
    installSelection({ selected: source, kind: "file", open: true });
    useWorkbenchStore.setState((state) => ({
      tabs: state.tabs.map((tab) => ({ ...tab, dirty: true })),
    }));
    render(
      <ExplorerReleasesOpenFiles.Provider value={true}>
        <ExplorerSurface />
      </ExplorerReleasesOpenFiles.Provider>,
    );
    const { menu } = openMenu();
    expect(
      within(menu).getByRole("menuitem", {
        name: "Save or revert this file before renaming or recycling it.",
      }),
    ).toHaveAttribute("aria-disabled", "true");
  });

  it("closes safely when the Project changes", async () => {
    installSelection({
      selected: childResource("root-a", "src/closed.ts"),
      kind: "file",
    });
    render(<ExplorerSurface />);
    openMenu();

    useWorkbenchStore.setState({
      project: { id: "project-b", name: "B", roots: [] },
      projectGeneration: 4,
      selectedRootId: null,
      selectedResource: null,
    });

    await vi.waitFor(() =>
      expect(screen.queryByRole("menu")).not.toBeInTheDocument(),
    );
  });
});
