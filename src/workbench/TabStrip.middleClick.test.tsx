import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";
import { DirtyTabDialog } from "./DirtyTabDialog";
import { TabStrip } from "./TabStrip";
import { EDITOR_CANVAS_KINDS } from "./workbenchWindowMode";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

function tab(
  id: string,
  kind: WorkbenchTab["kind"],
  position: number,
): WorkbenchTab {
  return {
    id,
    kind,
    title: id,
    position,
    pinned: false,
    dirty: false,
    ...(kind === "file" || kind === "diff"
      ? { resource: { rootId: "root-a", relativePath: `${id}.ts` } }
      : {}),
  };
}

function seed(tabs: WorkbenchTab[], activeTabId: string) {
  useWorkbenchStore.setState({
    tabs,
    activeTabId,
    project: { id: "project-a", name: "Project A", roots: [] },
    pendingCloseTabId: null,
  });
}

function state() {
  const { tabs, activeTabId, pendingCloseTabId } = useWorkbenchStore.getState();
  return { tabs, activeTabId, pendingCloseTabId };
}

function middleClick(target: HTMLElement) {
  fireEvent.mouseDown(target, { button: 1 });
  fireEvent.mouseUp(target, { button: 1 });
  fireEvent(
    target,
    new MouseEvent("auxclick", { button: 1, bubbles: true, cancelable: true }),
  );
}

describe("TabStrip middle click", () => {
  it.each(["file", "diff"] as const)(
    "closes a clean %s tab with the same successor as X",
    (kind) => {
      const tabs = [
        tab("first", "file", 0),
        tab("target", kind, 1),
        tab("last", "file", 2),
      ];
      seed(tabs, "target");
      render(<TabStrip kinds={EDITOR_CANVAS_KINDS} showNewChat={false} />);
      fireEvent.click(screen.getByRole("button", { name: "Close target" }));
      const afterX = state();
      expect(afterX).toEqual({
        tabs: [tabs[0], { ...tabs[2], position: 1 }],
        activeTabId: "last",
        pendingCloseTabId: null,
      });

      act(() => seed(tabs, "target"));
      middleClick(screen.getByRole("tab", { name: "target" }));
      expect(state()).toEqual(afterX);
    },
  );

  it.each([false, true])(
    "does not activate an inactive file before closing (dirty=%s)",
    async (dirty) => {
      const tabs = [
        tab("first", "file", 0),
        { ...tab("target", "file", 1), dirty },
        tab("last", "file", 2),
      ];
      seed(tabs, "first");
      render(
        <>
          <TabStrip kinds={EDITOR_CANVAS_KINDS} showNewChat={false} />
          <DirtyTabDialog />
        </>,
      );
      const activeIds: (string | null)[] = [];
      const unsubscribe = useWorkbenchStore.subscribe((next) =>
        activeIds.push(next.activeTabId),
      );
      try {
        await act(async () =>
          middleClick(screen.getByRole("tab", { name: "target" })),
        );
        expect(state()).toEqual({
          tabs: dirty ? tabs : [tabs[0], { ...tabs[2], position: 1 }],
          activeTabId: "first",
          pendingCloseTabId: dirty ? "target" : null,
        });
        expect(activeIds.every((id) => id === "first")).toBe(true);
        if (dirty) {
          expect(
            screen.getByRole("dialog", { name: "Save changes to target?" }),
          ).toBeInTheDocument();
          fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
          expect(state()).toEqual({
            tabs,
            activeTabId: "first",
            pendingCloseTabId: null,
          });
        }
      } finally {
        unsubscribe();
      }
    },
  );

  it.each([false, true])("leaves a pinned file alone (dirty=%s)", (dirty) => {
    const tabs = [
      tab("first", "file", 0),
      { ...tab("target", "file", 1), pinned: true, dirty },
    ];
    seed(tabs, "first");
    render(<TabStrip showNewChat={false} />);
    expect(
      screen.queryByRole("button", { name: "Close target" }),
    ).not.toBeInTheDocument();
    middleClick(screen.getByRole("tab", { name: "target" }));
    expect(state()).toEqual({
      tabs,
      activeTabId: "first",
      pendingCloseTabId: null,
    });
  });

  it("leaves a tab with a disabled X alone", () => {
    const tabs = [tab("only chat", "chat", 0), tab("file", "file", 1)];
    seed(tabs, "file");
    render(<TabStrip showNewChat={false} />);
    expect(
      screen.getByRole("button", { name: "Archive only chat" }),
    ).toBeDisabled();
    expect(useWorkbenchStore.getState().error).toBeNull();
    middleClick(screen.getByRole("tab", { name: "only chat" }));
    expect(state()).toEqual({
      tabs,
      activeTabId: "file",
      pendingCloseTabId: null,
    });
    expect(useWorkbenchStore.getState().error).toBeNull();
  });

  it.each([
    "projectTransitioning",
    "fileRenameInFlight",
    "resourceTrashInFlight",
  ] as const)("honors the store's %s close guard", (guard) => {
    const tabs = [tab("first", "file", 0), tab("target", "file", 1)];
    seed(tabs, "first");
    useWorkbenchStore.setState({ [guard]: true });
    render(<TabStrip showNewChat={false} />);
    middleClick(screen.getByRole("tab", { name: "target" }));
    expect(state()).toEqual({
      tabs,
      activeTabId: "first",
      pendingCloseTabId: null,
    });
  });

  it("left click only activates and right click leaves the tab alone", async () => {
    const tabs = [tab("first", "file", 0), tab("settings", "settings", 1)];
    seed(tabs, "first");
    render(<TabStrip kinds={EDITOR_CANVAS_KINDS} showNewChat={false} />);
    const target = screen.getByRole("tab", { name: "settings" });
    fireEvent(target, new MouseEvent("auxclick", { button: 2, bubbles: true }));
    expect(state()).toEqual({
      tabs,
      activeTabId: "first",
      pendingCloseTabId: null,
    });
    await act(async () => fireEvent.click(target, { button: 0 }));
    expect(state()).toEqual({
      tabs,
      activeTabId: "settings",
      pendingCloseTabId: null,
    });
  });

  it("prevents middle-button autoscroll across the tab, including X and pinned tabs", () => {
    const tabs = [
      tab("first", "file", 0),
      { ...tab("pinned", "file", 1), pinned: true },
    ];
    seed(tabs, "first");
    render(<TabStrip showNewChat={false} />);
    const first = screen.getByRole("tab", { name: "first" });
    for (const target of [
      first,
      first.parentElement!,
      screen.getByRole("button", { name: "Close first" }),
      screen.getByRole("tab", { name: "pinned" }),
    ]) {
      expect(fireEvent.mouseDown(target, { button: 1 })).toBe(false);
      expect(state()).toEqual({
        tabs,
        activeTabId: "first",
        pendingCloseTabId: null,
      });
      expect(fireEvent.mouseDown(target, { button: 0 })).toBe(true);
      expect(fireEvent.mouseDown(target, { button: 2 })).toBe(true);
    }
  });
});
