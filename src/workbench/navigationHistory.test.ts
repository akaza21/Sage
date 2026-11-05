import { describe, expect, it } from "vitest";
import {
  createNavigationHistory,
  MAX_NAVIGATION_ENTRIES,
  type NavigationEntry,
} from "./navigationHistory";

const place = (tabId: string): NavigationEntry => ({
  projectId: "project",
  tabId,
  mode: "agent",
  settingsPage: null,
  sidebarView: "explorer",
});

describe("workbench navigation history", () => {
  it("travels both ways and discards forward history after a new navigation", () => {
    const history = createNavigationHistory();
    for (const id of ["a", "b", "c"]) history.getState().record(place(id));
    expect(history.getState().step(-1)?.tabId).toBe("b");
    expect(history.getState().step(-1)?.tabId).toBe("a");
    expect(history.getState().step(-1)).toBeNull();
    expect(history.getState().step(1)?.tabId).toBe("b");
    history.getState().record(place("d"));
    expect(history.getState().step(1)).toBeNull();
    expect(history.getState().step(-1)?.tabId).toBe("b");
  });

  it("caps the whole history, including the current and forward entries", () => {
    const history = createNavigationHistory();
    for (let id = 0; id < MAX_NAVIGATION_ENTRIES + 20; id++)
      history.getState().record(place(String(id)));
    expect(history.getState().entries).toHaveLength(MAX_NAVIGATION_ENTRIES);
    for (let id = 0; id < MAX_NAVIGATION_ENTRIES; id++)
      history.getState().step(-1);
    expect(history.getState().entries[history.getState().index].tabId).toBe(
      "20",
    );
    history.getState().record(place("new"));
    expect(history.getState().entries.map((entry) => entry.tabId)).toEqual([
      "20",
      "new",
    ]);
  });

  it("prunes deleted or archived destinations in both directions, including the current entry", () => {
    const history = createNavigationHistory();
    for (const id of ["a", "deleted", "b", "archived", "c"])
      history.getState().record(place(id));
    history
      .getState()
      .prune((entry) => !["deleted", "archived"].includes(entry.tabId!));
    expect(history.getState().step(-1)?.tabId).toBe("b");
    history.getState().prune((entry) => entry.tabId !== "b");
    expect(history.getState().step(1)?.tabId).toBe("c");
    expect(history.getState().step(-1)?.tabId).toBe("a");
    expect(history.getState().entries.map((entry) => entry.tabId)).toEqual([
      "a",
      "c",
    ]);
  });

  it("records Settings open/close, pane changes and window modes without duplicate entries or content", () => {
    const history = createNavigationHistory();
    const initial = place("a");
    history.getState().record(initial);
    history.getState().record({ ...initial });
    history.getState().record({ ...initial, settingsPage: "Appearance" });
    history.getState().record(initial);
    history
      .getState()
      .record({ ...initial, mode: "editor", sidebarView: "search" });
    expect(history.getState().entries).toHaveLength(4);
    expect(history.getState().step(-1)).toEqual(initial);
    expect(history.getState().step(-1)?.settingsPage).toBe("Appearance");
    initial.tabId = "mutated";
    expect(history.getState().step(-1)?.tabId).toBe("a");
  });
});
