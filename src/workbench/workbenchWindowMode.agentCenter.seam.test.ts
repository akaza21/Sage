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

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () =>
    createElement("section", { "aria-label": "Agent chat" }, "Active chat"),
}));

let compactViewport = false;

vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => compactViewport,
}));

const initialState = useWorkbenchStore.getState();
const project = { id: "project-a", name: "A", roots: [] };

afterEach(() => {
  cleanup();
  compactViewport = false;
  useWorkbenchStore.setState(initialState, true);
});

describe("Agent center workbench window mode seam", () => {
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
});
