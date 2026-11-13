import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";
import { TabStrip } from "./TabStrip";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

const fileTab = (index: number): WorkbenchTab => ({
  id: `file:${index}`,
  kind: "file",
  title: `file${index}.ts`,
  position: index,
  pinned: false,
  dirty: false,
  resource: { rootId: "root-a", relativePath: `src/file${index}.ts` },
});

function renderStrip(activeIndex: number) {
  const activateTab = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    tabs: [0, 1, 2].map(fileTab),
    activeTabId: `file:${activeIndex}`,
    activateTab,
  });
  render(<TabStrip />);
  return activateTab;
}

describe("TabStrip keyboard", () => {
  it("is a tab list with one Tab stop on the active tab", () => {
    renderStrip(1);
    const list = screen.getByRole("tablist", { name: "Tabs" });
    expect(list).toBeInTheDocument();
    expect(screen.getAllByRole("tab").map((tab) => tab.tabIndex)).toEqual([
      -1, 0, -1,
    ]);
  });

  it("moves focus with the arrows without opening the tab", () => {
    const activateTab = renderStrip(1);
    const active = screen.getByRole("tab", { name: "file1.ts" });
    active.focus();
    fireEvent.keyDown(active, { key: "ArrowRight" });
    expect(screen.getByRole("tab", { name: "file2.ts" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
    expect(screen.getByRole("tab", { name: "file0.ts" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "End" });
    expect(screen.getByRole("tab", { name: "file2.ts" })).toHaveFocus();
    expect(activateTab).not.toHaveBeenCalled();
  });

  it("leaves Enter and Space to open the focused tab", () => {
    const activateTab = renderStrip(0);
    const tab = screen.getByRole("tab", { name: "file2.ts" });
    tab.focus();
    // Not prevented, so the button's native activation opens the tab.
    expect(fireEvent.keyDown(tab, { key: "Enter" })).toBe(true);
    expect(fireEvent.keyDown(tab, { key: " " })).toBe(true);
    fireEvent.click(tab);
    expect(activateTab).toHaveBeenCalledWith("file:2");
  });

  it("moves the Tab stop when another tab becomes active", () => {
    renderStrip(0);
    act(() => useWorkbenchStore.setState({ activeTabId: "file:2" }));
    expect(screen.getAllByRole("tab").map((tab) => tab.tabIndex)).toEqual([
      -1, -1, 0,
    ]);
  });
});
