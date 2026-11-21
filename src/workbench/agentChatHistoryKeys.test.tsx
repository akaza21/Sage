import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { clearAgentChatOwner, pinAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";
import { resetAgentChatHistory } from "./agentChatHistory";
import { TitleBar } from "./TitleBar";
import { useHistoryKeys } from "./useHistoryKeys";

vi.mock("../ui/shortcutLabel", () => ({
  shortcutLabel: (key: string) => `Ctrl+${key.toUpperCase()}`,
  usesMetaModifier: () => false,
}));

const initialState = useWorkbenchStore.getState();

const chat = (id: string): WorkbenchTab => ({
  id,
  kind: "chat",
  title: `Chat: ${id}`,
  position: 0,
  pinned: false,
  dirty: false,
});

const layouts: Record<string, WorkbenchTab[]> = {
  "project-a": [chat("a1"), chat("a2")],
  "project-b": [chat("b1")],
};

function showProject(projectId: string, activeTabId: string) {
  useWorkbenchStore.setState({
    project: { id: projectId, name: projectId, roots: [] },
    tabs: layouts[projectId],
    activeTabId,
  });
}

function AgentWindow() {
  useHistoryKeys("agent");
  return <TitleBar mode="agent" onModeChange={() => undefined} />;
}

const shown = () => {
  const state = useWorkbenchStore.getState();
  return `${state.project?.id}/${state.activeTabId}`;
};
const settle = () => act(() => new Promise((resolve) => setTimeout(resolve)));

beforeEach(() => {
  useWorkbenchStore.setState({
    phase: "ready",
    projectTransitioning: false,
    projects: [
      { id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 },
      { id: "project-b", name: "B", rootCount: 1, lastOpenedAt: 2 },
    ],
    // The native commits these would wait for are outside this test: an
    // activation or a project switch lands as soon as it is asked for.
    activateTab: async (tabId) => {
      useWorkbenchStore.setState({ activeTabId: tabId });
    },
  });
  showProject("project-a", "a1");
});

afterEach(() => {
  cleanup();
  resetAgentChatHistory();
  clearAgentChatOwner();
  useWorkbenchStore.setState(initialState, true);
});

it("goes back and forward between chats on Alt+arrows and the mouse buttons", async () => {
  render(<AgentWindow />);
  act(() => useWorkbenchStore.setState({ activeTabId: "a2" }));

  fireEvent.keyDown(document.body, { key: "ArrowLeft", altKey: true });
  await settle();
  expect(shown()).toBe("project-a/a1");

  fireEvent.mouseUp(window, { button: 4 });
  await settle();
  expect(shown()).toBe("project-a/a2");
  expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
});

it("keeps the way back while a pending send holds the project, then takes it", async () => {
  render(<AgentWindow />);
  act(() => showProject("project-b", "b1"));
  useWorkbenchStore.setState({ sending: true });

  fireEvent.keyDown(document.body, { key: "ArrowLeft", altKey: true });
  await settle();
  expect(shown()).toBe("project-b/b1");
  expect(screen.getByRole("button", { name: "Back" })).toBeEnabled();

  useWorkbenchStore.setState({
    sending: false,
    switchProject: async (projectId) => showProject(projectId, "a2"),
  });
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  await settle();
  expect(shown()).toBe("project-a/a1");
  expect(screen.getByRole("button", { name: "Forward" })).toBeEnabled();
});

it("keeps Back when a refused switch leaves the retained chat showing", async () => {
  render(<AgentWindow />);
  act(() => {
    useWorkbenchStore.setState({ activeTabId: "a2" });
    pinAgentChatOwner("a2");
  });
  act(() =>
    useWorkbenchStore.setState({
      tabs: [
        ...layouts["project-a"],
        {
          id: "file:notes",
          kind: "file",
          title: "notes.md",
          position: 2,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "notes.md" },
        },
      ],
      activeTabId: "file:notes",
      activateTab: initialState.activateTab,
      flushActiveEditor: async () => {
        throw new Error("disk unavailable");
      },
    }),
  );

  fireEvent.keyDown(document.body, { key: "ArrowLeft", altKey: true });
  await settle();

  expect(useWorkbenchStore.getState().activeTabId).toBe("file:notes");
  expect(screen.getByRole("button", { name: "Back" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
});
