import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { childResource, resourceKey } from "../../domain/resourceIdentity";
import { ExplorerSurface } from "./ExplorerSurface";

vi.mock("./FileKindIcon", () => ({ FileKindIcon: () => <span /> }));

Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
  configurable: true,
  value: vi.fn(),
});

const initialState = useWorkbenchStore.getState();
const root = childResource("root-a");
const file = childResource("root-a", "notes.md");
const other = childResource("root-a", "todo.md");

beforeEach(() => {
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
    selectedRootId: null,
    tabs: [],
    activeEditor: null,
    directoryListings: {
      [resourceKey(root)]: {
        phase: "ready",
        value: {
          resource: root,
          entries: [
            { name: "notes.md", resource: file, kind: "file", byteLen: 4 },
            { name: "todo.md", resource: other, kind: "file", byteLen: 4 },
          ],
          truncated: false,
        },
      },
    },
    expandedDirectories: {},
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

const fileRow = () =>
  screen.getByRole("treeitem", { name: "Open file notes.md" });
const menu = () => screen.getByRole("menu", { name: "Explorer actions" });

it("selects a File without opening it and offers its actions at its row (ED-18)", async () => {
  const openFile = vi.fn(async () => undefined);
  useWorkbenchStore.setState({ openFile });
  render(<ExplorerSurface />);

  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    fileRow().dispatchEvent(event);
  });
  // The webview's own menu (Reload, Inspect) does not show over it.
  expect(event.defaultPrevented).toBe(true);
  expect(useWorkbenchStore.getState().selectedResource).toEqual(file);
  expect(openFile).not.toHaveBeenCalled();
  expect(menu()).toBeVisible();
  expect(screen.getByRole("menuitem", { name: "Rename File" })).toBeVisible();
  await waitFor(() =>
    expect(
      screen.getByRole("menuitem", { name: "Add folder root" }),
    ).toHaveFocus(),
  );

  // Escape gives the focus back to the row, not the "…" button.
  fireEvent.keyDown(menu(), { key: "Escape" });
  await waitFor(() => expect(fileRow()).toHaveFocus());
});

it("opens on the root's heading too, with what applies there", () => {
  render(<ExplorerSurface />);
  fireEvent.contextMenu(screen.getByRole("button", { name: "A" }));
  expect(useWorkbenchStore.getState().selectedResource).toEqual(root);
  expect(menu()).toBeVisible();
  expect(screen.queryByRole("menuitem", { name: "Rename File" })).toBeNull();
});

it("opens nothing, and no webview menu, while a rename keeps the selection", () => {
  useWorkbenchStore.setState({ fileRenameInFlight: true });
  render(<ExplorerSurface />);
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    fileRow().dispatchEvent(event);
  });
  expect(event.defaultPrevented).toBe(true);
  expect(screen.queryByRole("menu", { name: "Explorer actions" })).toBeNull();
});

it("moves back to the button when the button is pressed", () => {
  render(<ExplorerSurface />);
  fireEvent.contextMenu(fileRow());
  expect(menu()).toBeVisible();
  const more = screen.getByRole("button", { name: "More Explorer actions" });
  fireEvent.click(more);
  expect(menu()).toBeVisible();
  fireEvent.click(more);
  expect(screen.queryByRole("menu", { name: "Explorer actions" })).toBeNull();
});

const otherRow = () =>
  screen.getByRole("treeitem", { name: "Open file todo.md" });

// What a real right-click sends: the press first (outside an open menu),
// then the context menu.
function rightClick(target: HTMLElement) {
  fireEvent.pointerDown(target, { button: 2 });
  fireEvent.mouseDown(target, { button: 2 });
  fireEvent.contextMenu(target);
}

it("keeps the focus in the menu when another row is right-clicked", async () => {
  render(<ExplorerSurface />);
  rightClick(fileRow());
  await waitFor(() =>
    expect(
      screen.getByRole("menuitem", { name: "Add folder root" }),
    ).toHaveFocus(),
  );
  rightClick(otherRow());
  expect(useWorkbenchStore.getState().selectedResource).toEqual(other);
  // Past the frame the first close would have handed the focus back on.
  await act(() => new Promise((resolve) => setTimeout(resolve, 50)));
  expect(menu()).toBeVisible();
  expect(menu()).toContainElement(document.activeElement as HTMLElement);
});

it("opens from the keyboard's context-menu key without a contextmenu event", async () => {
  render(<ExplorerSurface />);
  fireEvent.keyDown(fileRow(), { key: "F10", shiftKey: true });
  expect(menu()).toBeVisible();
  // The menu is the row's, not the button's.
  expect(
    screen.getByRole("button", { name: "More Explorer actions" }),
  ).toHaveAttribute("aria-expanded", "false");
  fireEvent.keyDown(menu(), { key: "Escape" });
  await waitFor(() => expect(fileRow()).toHaveFocus());
  fireEvent.keyDown(screen.getByRole("button", { name: "A" }), {
    key: "ContextMenu",
  });
  expect(useWorkbenchStore.getState().selectedResource).toEqual(root);
  expect(menu()).toBeVisible();
});

it("gives the focus to the button when the row is gone", async () => {
  render(<ExplorerSurface />);
  rightClick(otherRow());
  act(() => {
    useWorkbenchStore.setState((state) => ({
      directoryListings: {
        [resourceKey(root)]: {
          phase: "ready",
          value: {
            ...(
              state.directoryListings[resourceKey(root)] as {
                value: { resource: typeof root; truncated: boolean };
              }
            ).value,
            entries: [
              { name: "notes.md", resource: file, kind: "file", byteLen: 4 },
            ],
          },
        },
      },
    }));
  });
  fireEvent.keyDown(menu(), { key: "Escape" });
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "More Explorer actions" }),
    ).toHaveFocus(),
  );
});
