import { afterEach, describe, expect, it, vi } from "vitest";

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
  useWorkbenchStore.setState(initialState, true);
  closeQuickOpen();
});

const labels = () => paletteCommands(false).map((command) => command.label);

describe("palette commands", () => {
  it("offers the File editor's commands only while one is showing", () => {
    expect(labels()).not.toContain("Find");
    const runner = vi.fn();
    const unregister = registerFileSurfaceCommands(runner);
    const find = paletteCommands(false).find((item) => item.label === "Find");
    expect(find?.shortcut).toBe("Ctrl+F");
    if (typeof find?.run === "function") find.run();
    expect(runner).toHaveBeenCalledWith("find");
    unregister();
    expect(labels()).not.toContain("Find");
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
