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
  AGENT_CENTER_CHAT_SCAN_LIMIT,
  type AgentCenterScanTab,
  agentCenterChatAction,
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

  it("returns the center to Chat when a File or Diff tab is activated while Agent stays selected", async () => {
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
          id: "diff:d",
          kind: "diff",
          title: "changed.ts",
          position: 1,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/changed.ts" },
        },
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 2,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "file:a",
      loadEditor: vi.fn(async () => undefined),
      loadDiff: vi.fn(async () => undefined),
      activateTab,
    });
    render(createElement(Workbench));

    const modeToggle = screen.getByRole("radiogroup", { name: "Window mode" });
    const agentControl = within(modeToggle).getByRole("radio", {
      name: "Agent",
    });
    fireEvent.click(agentControl);

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenCalledWith("chat:b");
    });
    await vi.waitFor(() => {
      expect(screen.getByRole("region", { name: "Agent chat" }));
    });

    const tabStrip = within(
      screen.getByRole("navigation", { name: "Open tabs" }),
    );
    fireEvent.click(tabStrip.getByRole("tab", { name: /changed\.ts/ }));

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenLastCalledWith("chat:b");
    });
    expect(activateTab).toHaveBeenNthCalledWith(2, "diff:d");
    expect(agentControl).toBeChecked();
    expect(screen.getByRole("region", { name: "Agent chat" }));

    fireEvent.click(tabStrip.getByRole("tab", { name: /current\.ts/ }));

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenCalledTimes(5);
    });
    expect(activateTab).toHaveBeenNthCalledWith(2, "diff:d");
    expect(activateTab).toHaveBeenNthCalledWith(4, "file:a");
    expect(activateTab).toHaveBeenLastCalledWith("chat:b");
    expect(agentControl).toBeChecked();
    expect(screen.queryByText("Loading src/current.ts…")).toBeNull();
    expect(screen.getByRole("region", { name: "Agent chat" }));
  });

  it("keeps the compact center interactive while tabs change during Agent mode", async () => {
    compactViewport = true;
    const activateTab = vi.fn(async (tabId: string) => {
      useWorkbenchStore.setState({ activeTabId: tabId });
    });
    useWorkbenchStore.setState({
      project,
      sidebarOpen: true,
      sidebarView: "explorer",
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
    const { container } = render(createElement(Workbench));
    const center = container.querySelector(".center-region");
    expect(center).toHaveAttribute("inert");

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    await vi.waitFor(() => {
      expect(screen.getByRole("region", { name: "Agent chat" }));
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(center).not.toHaveAttribute("inert");
    expect(center).not.toHaveAttribute("aria-hidden");

    fireEvent.click(
      within(screen.getByRole("navigation", { name: "Open tabs" })).getByRole(
        "tab",
        { name: /current\.ts/ },
      ),
    );

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenLastCalledWith("chat:b");
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(center).not.toHaveAttribute("inert");
    expect(center).not.toHaveAttribute("aria-hidden");
    expect(screen.getByRole("region", { name: "Agent chat" }));
  });

  describe("agentCenterChatAction scan bounds", () => {
    it("declares a finite positive scan bound", () => {
      expect(Number.isInteger(AGENT_CENTER_CHAT_SCAN_LIMIT)).toBe(true);
      expect(AGENT_CENTER_CHAT_SCAN_LIMIT).toBeGreaterThan(0);
      expect(AGENT_CENTER_CHAT_SCAN_LIMIT).toBeLessThanOrEqual(1024);
    });

    it("activates the first chat inside the bound without depending on later tabs", () => {
      const total = AGENT_CENTER_CHAT_SCAN_LIMIT * 3;
      const tabs: AgentCenterScanTab[] = Array.from(
        { length: total },
        (_, index): AgentCenterScanTab =>
          index === 5
            ? { id: "chat:first", kind: "chat" }
            : { id: `file:${index}`, kind: "file" },
      );
      tabs[AGENT_CENTER_CHAT_SCAN_LIMIT] = {
        id: "chat:outside",
        kind: "chat",
      };

      expect(agentCenterChatAction(tabs, "file:0")).toEqual({
        action: "activate",
        tabId: "chat:first",
      });
    });

    it("falls back to creating a chat when none exists inside the bound", () => {
      const total = AGENT_CENTER_CHAT_SCAN_LIMIT + 16;
      const tabs: AgentCenterScanTab[] = Array.from(
        { length: total },
        (_, index): AgentCenterScanTab =>
          index === AGENT_CENTER_CHAT_SCAN_LIMIT
            ? { id: "chat:outside", kind: "chat" }
            : { id: `file:${index}`, kind: "file" },
      );

      expect(agentCenterChatAction(tabs, "file:0")).toEqual({
        action: "create",
      });
    });

    it("treats an active chat inside the bound as already centered", () => {
      const tabs: AgentCenterScanTab[] = [
        { id: "file:0", kind: "file" },
        { id: "chat:active", kind: "chat" },
        { id: "file:2", kind: "file" },
      ];

      expect(agentCenterChatAction(tabs, "chat:active")).toEqual({
        action: "none",
      });
    });

    it("avoids unrestricted tab walks and untyped kinds in the module source", async () => {
      const source = await readFile(
        path.join(srcRoot, "workbench/workbenchWindowMode.ts"),
        "utf8",
      );
      expect(source).not.toMatch(/\.find\(/);
      expect(source).not.toMatch(/kind: string\b/);
    });
  });
});
