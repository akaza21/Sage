import { afterEach, describe, expect, it, vi } from "vitest";

import { notePlace, resetEditorHistory } from "../../app/editorHistory";
import { useWorkbenchStore } from "../../app/store";
import { registerFileSurfaceCommands } from "../files/fileSurfaceCommandBus";
import { filterCommands, paletteCommands } from "./paletteCommands";
import {
  closeQuickOpen,
  openCommandPalette,
  openQuickOpen,
  useQuickOpenStore,
} from "./quickOpenState";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  resetEditorHistory();
  useWorkbenchStore.setState(initialState, true);
  closeQuickOpen();
});

const labels = () => paletteCommands(false).map((command) => command.label);

describe("palette commands", () => {
  it("offers the File editor's commands only while they would do something", () => {
    expect(labels()).not.toContain("Find");
    const run = vi.fn();
    let previewing = false;
    const unregister = registerFileSurfaceCommands({
      available: (command) => command === "save" || !previewing,
      run,
    });
    try {
      const find = paletteCommands(false).find((item) => item.label === "Find");
      expect(find?.shortcut).toBe("Ctrl+F");
      if (typeof find?.run === "function") find.run();
      expect(run).toHaveBeenCalledWith("find");
      previewing = true;
      expect(labels()).not.toContain("Find");
      expect(labels()).not.toContain("Go to Line…");
      expect(labels()).toContain("Save File");
    } finally {
      unregister();
    }
    expect(labels()).not.toContain("Save File");
  });

  it("offers Go Back only when there is a place to go back to", () => {
    expect(labels()).not.toContain("Go Back");
    useWorkbenchStore.setState({
      project: { id: "project" } as ReturnType<
        typeof useWorkbenchStore.getState
      >["project"],
    });
    const resource = (relativePath: string) => ({ rootId: "r", relativePath });
    notePlace({
      projectId: "project",
      resource: resource("a"),
      line: 1,
      column: 1,
    });
    notePlace({
      projectId: "project",
      resource: resource("b"),
      line: 1,
      column: 1,
    });
    const back = paletteCommands(false).find(
      (item) => item.label === "Go Back",
    );
    expect(back?.shortcut).toBe("Alt+Left");
    expect(labels()).not.toContain("Go Forward");
    expect(
      paletteCommands(true).find((item) => item.label === "Go Back")?.shortcut,
    ).toBe("Ctrl+-");
  });

  it("shows macOS shortcuts on macOS", () => {
    const quickOpen = paletteCommands(true).find(
      (item) => item.id === "quickOpen",
    );
    expect(quickOpen?.shortcut).toBe("Cmd+P");
  });

  it("offers Close Editor only when an editor tab is active", () => {
    expect(labels()).not.toContain("Close Editor");
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "file:a",
          kind: "file",
          title: "a.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: { rootId: "root", relativePath: "a.ts" },
        },
      ],
      activeTabId: "file:a",
    });
    expect(labels()).toContain("Close Editor");
  });

  it("leaves out Open Settings while Settings is the active tab", () => {
    expect(labels()).toContain("Open Settings");
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "settings:appearance",
          kind: "settings",
          title: "Settings",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "settings:appearance",
    });
    expect(labels()).not.toContain("Open Settings");
    expect(labels()).toContain("Close Editor");
  });

  it("matches every typed word, in any order", () => {
    const commands = paletteCommands(false);
    expect(
      filterCommands(commands, "control show").map((item) => item.label),
    ).toEqual(["Show Source Control"]);
    expect(filterCommands(commands, "  ")).toHaveLength(commands.length);
  });
});

describe("opening Quick Open", () => {
  it("opens on Files, keeps an open box as it is, and opens commands afresh", () => {
    openQuickOpen();
    const first = useQuickOpenStore.getState();
    expect(first).toMatchObject({ open: true, seed: "" });
    openQuickOpen();
    expect(useQuickOpenStore.getState().opening).toBe(first.opening);
    openCommandPalette();
    expect(useQuickOpenStore.getState()).toMatchObject({
      open: true,
      seed: ">",
      opening: first.opening + 1,
    });
  });
});
