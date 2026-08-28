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

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () =>
    createElement("section", { "aria-label": "Agent chat" }, "Active chat"),
}));

let compactViewport = false;

vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => compactViewport,
}));

const srcRoot = path.resolve(import.meta.dirname, "..");
const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  localStorage.clear();
  compactViewport = false;
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
    expect(
      screen.queryByRole("complementary", { name: "Projects and chats" }),
    ).toBeNull();
    expect(
      screen.queryByRole("complementary", { name: "Agent artifacts" }),
    ).toBeNull();
  });

  it("rehydrates Agent mode from the existing bounded workbench layout", () => {
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 272,
        agentChatSplitPercent: 50,
        windowMode: "agent",
      }),
    );

    render(createElement(Workbench));

    expect(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    ).toBeChecked();
  });

  it("keeps Agent artifacts on the right and hides Editor chrome in Agent mode", () => {
    useWorkbenchStore.setState({
      sidebarOpen: false,
      tabs: [
        {
          id: "file:a",
          kind: "file",
          title: "current.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/current.ts" },
        },
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 1,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "file:a",
      loadEditor: vi.fn(async () => undefined),
    });
    render(createElement(Workbench));

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    expect(
      screen.getByRole("complementary", { name: "Projects and chats" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("complementary", { name: "Agent artifacts" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("navigation", { name: "Agent artifact tabs" }),
    ).toBeNull();
    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Explorer" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Search" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Source Control" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Problems" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Outline" })).toBeNull();
  });

  it("places Editor project tools on the left of the file canvas", () => {
    const { container } = render(createElement(Workbench));
    const body = container.querySelector(".workbench__body");
    const tools = screen.getByRole("complementary", { name: "Project tools" });
    expect(body?.firstElementChild).toBe(tools);
    expect(
      screen.queryByRole("complementary", { name: "Projects and chats" }),
    ).toBeNull();
  });

  it("keeps one file-or-diff tab strip across both window modes", () => {
    render(createElement(Workbench));

    expect(
      screen.getAllByRole("navigation", { name: "Open tabs" }),
    ).toHaveLength(1);
    expect(
      screen.queryByRole("navigation", { name: "Agent artifact tabs" }),
    ).toBeNull();

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    expect(screen.queryByRole("navigation", { name: "Open tabs" })).toBeNull();
    expect(
      screen.queryByRole("navigation", { name: "Agent artifact tabs" }),
    ).toBeNull();
  });

  it("keeps desktop out of Workbench composition", async () => {
    const source = await readFile(
      path.join(srcRoot, "workbench/Workbench.tsx"),
      "utf8",
    );
    expect(source).not.toMatch(/from ["'][^"']*desktop["']/);
  });
});
