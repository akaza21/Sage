import { createElement } from "react";
import {
  act,
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

async function failProjectTransition(label: string) {
  useWorkbenchStore.setState({ projectTransitioning: true });
  useWorkbenchStore.setState({
    projectTransitioning: false,
    error: `${label} failed`,
    projectGeneration: useWorkbenchStore.getState().projectGeneration + 1,
  });
}

async function pumpTransitionResets(times: number) {
  for (let index = 0; index < times; index += 1) {
    await act(async () => {
      useWorkbenchStore.setState({ projectTransitioning: true, error: null });
    });
    await act(async () => {
      useWorkbenchStore.setState({ projectTransitioning: false });
    });
  }
}

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

  it("restores Chat as the Agent center once a project transition completes", async () => {
    const activateTab = vi.fn(async (tabId: string) => {
      if (useWorkbenchStore.getState().projectTransitioning) return;
      useWorkbenchStore.setState({ activeTabId: tabId });
    });
    const createChat = vi.fn(async () => undefined);
    const nextProject = { id: "project-b", name: "B", roots: [] };
    useWorkbenchStore.setState({
      project,
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "Chat: A",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat:a",
      activateTab,
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
      expect(
        screen.getByRole("region", { name: "Agent chat" }),
      ).toBeInTheDocument();
    });

    await act(async () => {
      useWorkbenchStore.setState({ projectTransitioning: true });
    });
    await act(async () => {
      useWorkbenchStore.setState({
        project: nextProject,
        projectGeneration: initialState.projectGeneration + 1,
        tabs: [
          {
            id: "file:b",
            kind: "file",
            title: "after.ts",
            position: 0,
            pinned: false,
            dirty: false,
            resource: { rootId: "root-b", relativePath: "src/after.ts" },
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
        activeTabId: "file:b",
      });
    });

    expect(activateTab).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().activeTabId).toBe("file:b");
    expect(screen.queryByRole("region", { name: "Agent chat" })).toBeNull();

    await act(async () => {
      useWorkbenchStore.setState({
        projectTransitioning: false,
        projectGeneration: initialState.projectGeneration + 2,
      });
    });

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenLastCalledWith("chat:b");
      expect(useWorkbenchStore.getState().activeTabId).toBe("chat:b");
      expect(
        screen.getByRole("region", { name: "Agent chat" }),
      ).toBeInTheDocument();
    });
    expect(createChat).not.toHaveBeenCalled();
  });

  it("does not re-create a Chat when a failed creation only resets the transition flag", async () => {
    const createChat = vi.fn(() => failProjectTransition("Chat creation"));
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
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
    });
    await pumpTransitionResets(3);
    expect(createChat).toHaveBeenCalledTimes(1);
  });

  it("does not re-activate a failed Chat when the transition flag resets", async () => {
    const activateTab = vi.fn((tabId: string) => {
      if (tabId !== "chat:b") {
        return Promise.resolve();
      }
      return failProjectTransition("Chat activation");
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

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenCalledTimes(1);
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
    });
    await pumpTransitionResets(3);
    expect(activateTab).toHaveBeenCalledTimes(1);
  });
});
