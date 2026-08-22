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

const project = { id: "project-a", name: "A", roots: [] };

afterEach(() => {
  cleanup();
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

  it("activates an existing Chat tab as the center when Agent mode takes over a File canvas", async () => {
    const activateTab = vi.fn(async (tabId: string) => {
      useWorkbenchStore.setState({ activeTabId: tabId });
    });
    useWorkbenchStore.setState({
      project,
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
      activateTab,
    });
    render(createElement(Workbench));
    expect(screen.getByText("Loading src/current.ts…")).toBeInTheDocument();

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    await vi.waitFor(() => {
      expect(
        screen.getByRole("region", { name: "Agent chat" }),
      ).toBeInTheDocument();
    });
    expect(screen.queryByText("Loading src/current.ts…")).toBeNull();
    expect(activateTab).toHaveBeenCalledWith("chat:b");
  });

  it("creates a Chat for the Agent center when no Chat tab exists to activate", async () => {
    const createChat = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      project,
      tabs: [
        {
          id: "diff:a",
          kind: "diff",
          title: "changed.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/changed.ts" },
        },
      ],
      activeTabId: "diff:a",
      loadDiff: vi.fn(async () => undefined),
      createChat,
    });
    render(createElement(Workbench));

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    await vi.waitFor(() => {
      expect(createChat).toHaveBeenCalledTimes(1);
    });
  });

  it("keeps the center interactive when Agent mode replaces an open compact Editor drawer", () => {
    compactViewport = true;
    useWorkbenchStore.setState({
      project,
      sidebarOpen: true,
      sidebarView: "explorer",
      tabs: [
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat:b",
    });
    const { container } = render(createElement(Workbench));
    const center = container.querySelector(".center-region");
    expect(center).toHaveAttribute("inert");
    expect(center).toHaveAttribute("aria-hidden", "true");
    expect(
      screen.getByRole("dialog", { name: "Explorer project tools" }),
    ).toBeInTheDocument();

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(center).not.toHaveAttribute("inert");
    expect(center).not.toHaveAttribute("aria-hidden");

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Editor" },
      ),
    );

    expect(
      screen.getByRole("dialog", { name: "Explorer project tools" }),
    ).toBeInTheDocument();
    expect(center).toHaveAttribute("inert");
    expect(center).toHaveAttribute("aria-hidden", "true");
  });
});
