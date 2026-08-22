import { readFile } from "node:fs/promises";
import path from "node:path";
import { createElement } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { Workbench } from "./Workbench";
import {
  DEFAULT_WORKBENCH_WINDOW_MODE,
  isWorkbenchWindowMode,
  WORKBENCH_WINDOW_MODES,
} from "./workbenchWindowMode";

vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => false,
}));

const srcRoot = path.resolve(import.meta.dirname, "..");
const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

describe("D0 workbench window mode seam", () => {
  it("offers exactly Agent and Editor with Editor as the default", () => {
    expect([...WORKBENCH_WINDOW_MODES]).toEqual(["agent", "editor"]);
    expect(DEFAULT_WORKBENCH_WINDOW_MODE).toBe("editor");
    expect(isWorkbenchWindowMode("agent")).toBe(true);
    expect(isWorkbenchWindowMode("editor")).toBe(true);
    expect(isWorkbenchWindowMode("split")).toBe(false);
    expect(isWorkbenchWindowMode(1)).toBe(false);
  });

  it("keeps the Editor tool rail by default and swaps to Agent chrome on toggle", () => {
    useWorkbenchStore.setState({ sidebarOpen: false });
    render(createElement(Workbench));

    const modeToggle = screen.getByRole("radiogroup", { name: "Window mode" });
    const agentControl = within(modeToggle).getByRole("radio", {
      name: "Agent",
    });
    const editorControl = within(modeToggle).getByRole("radio", {
      name: "Editor",
    });
    expect(editorControl).toBeChecked();
    expect(agentControl).not.toBeChecked();
    expect(
      screen.getByRole("complementary", { name: "Project tools" }),
    ).toBeInTheDocument();

    fireEvent.click(agentControl);

    expect(agentControl).toBeChecked();
    expect(editorControl).not.toBeChecked();
    expect(
      screen.queryByRole("complementary", { name: "Project tools" }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Explorer" })).toBeNull();
    expect(
      screen.getByRole("complementary", { name: "Projects and chats" }),
    ).toBeInTheDocument();

    fireEvent.click(editorControl);

    expect(editorControl).toBeChecked();
    expect(
      screen.getByRole("complementary", { name: "Project tools" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Explorer" }),
    ).toBeInTheDocument();
  });

  it("keeps one primary tab strip across both window modes", () => {
    render(createElement(Workbench));

    expect(
      screen.getAllByRole("navigation", { name: "Open tabs" }),
    ).toHaveLength(1);

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    expect(
      screen.getAllByRole("navigation", { name: "Open tabs" }),
    ).toHaveLength(1);
  });

  it("keeps desktop out of Workbench composition", async () => {
    const source = await readFile(
      path.join(srcRoot, "workbench/Workbench.tsx"),
      "utf8",
    );
    expect(source).not.toMatch(/from ["'][^"']*desktop["']/);
  });
});
