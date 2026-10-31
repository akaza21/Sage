import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { useQuickOpenStore } from "../features/quickOpen/quickOpenState";
import { EditorWelcome } from "./EditorWelcome";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

describe("EditorWelcome", () => {
  it("offers only working Editor and Agent destinations", () => {
    const onOpenAgent = vi.fn();
    useWorkbenchStore.setState({ sidebarOpen: false, sidebarView: "outline" });
    render(<EditorWelcome onOpenAgent={onOpenAgent} />);

    fireEvent.click(screen.getByRole("button", { name: "Browse files" }));
    expect(useWorkbenchStore.getState()).toMatchObject({
      sidebarOpen: true,
      sidebarView: "explorer",
    });

    fireEvent.click(screen.getByRole("button", { name: "Search files" }));
    expect(useQuickOpenStore.getState().open).toBe(true);
    useQuickOpenStore.setState({ open: false });

    fireEvent.click(screen.getByRole("button", { name: "Open Agent" }));
    expect(onOpenAgent).toHaveBeenCalledOnce();
  });

  it("moves focus into an Explorer that is already open", async () => {
    useWorkbenchStore.setState({ sidebarOpen: true, sidebarView: "explorer" });
    render(
      <>
        <div role="tree" aria-label="Project files">
          <button role="treeitem">README.md</button>
          <button role="treeitem" className="tree__row tree__row--selected">
            src
          </button>
        </div>
        <EditorWelcome onOpenAgent={vi.fn()} />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Browse files" }));
    await waitFor(() =>
      expect(screen.getByRole("treeitem", { name: "src" })).toHaveFocus(),
    );
  });

  it("waits for an Explorer that mounts after the click", async () => {
    render(<EditorWelcome onOpenAgent={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Browse files" }));
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const tree = document.createElement("div");
    tree.setAttribute("role", "tree");
    tree.setAttribute("aria-label", "Project files");
    const row = document.createElement("button");
    row.setAttribute("role", "treeitem");
    row.textContent = "README.md";
    tree.append(row);
    document.body.append(tree);
    try {
      await waitFor(() => expect(row).toHaveFocus());
    } finally {
      tree.remove();
    }
  });

  it("leaves focus alone when the user moved it before Explorer arrived", async () => {
    render(
      <>
        <input aria-label="Elsewhere" />
        <EditorWelcome onOpenAgent={vi.fn()} />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Browse files" }));
    const elsewhere = screen.getByRole("textbox", { name: "Elsewhere" });
    elsewhere.focus();
    const tree = document.createElement("div");
    tree.setAttribute("role", "tree");
    tree.setAttribute("aria-label", "Project files");
    const row = document.createElement("button");
    row.setAttribute("role", "treeitem");
    tree.append(row);
    document.body.append(tree);
    try {
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(elsewhere).toHaveFocus();
    } finally {
      tree.remove();
    }
  });

  it("waits for a usable row instead of the root heading or a disabled row", async () => {
    render(
      <>
        <section className="sidebar" tabIndex={-1}>
          <div role="tree" aria-label="Project files">
            <button className="root-tree__heading">project</button>
            <button role="treeitem" disabled>
              link
            </button>
          </div>
        </section>
        <EditorWelcome onOpenAgent={vi.fn()} />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Browse files" }));
    // The compact drawer focuses its panel when it opens.
    document.querySelector<HTMLElement>(".sidebar")!.focus();
    await new Promise((resolve) => requestAnimationFrame(resolve));
    await new Promise((resolve) => requestAnimationFrame(resolve));
    expect(screen.getByRole("button", { name: "project" })).not.toHaveFocus();
    const row = document.createElement("button");
    row.setAttribute("role", "treeitem");
    row.textContent = "README.md";
    screen.getByRole("tree").append(row);
    await waitFor(() => expect(row).toHaveFocus());
  });
});
