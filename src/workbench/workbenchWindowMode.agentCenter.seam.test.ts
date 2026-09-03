import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import type { AcpChatPage, ProjectGeneration } from "../domain/acp";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
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
const artifactOwner = {
  projectId: "project-a",
  chatId: "chat:b",
  sessionId: "session-b",
} as AgentArtifactOwner;
const projectListItem = {
  id: project.id,
  name: project.name,
  rootCount: 0,
  lastOpenedAt: 1,
};

function seedArtifactOwner(): void {
  const store = acpProjectionStoreFor(artifactOwner.chatId);
  const scope = {
    projectId: artifactOwner.projectId,
    projectGeneration: "0" as ProjectGeneration,
    chatId: artifactOwner.chatId,
  };
  store.getState().activateScope(scope);
  store.getState().applyAuthoritativePage(scope, {
    projectId: artifactOwner.projectId,
    chatId: artifactOwner.chatId,
    title: "Chat B",
    projectionRevision: "1",
    currentSessionId: artifactOwner.sessionId,
    sessions: [],
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage);
}

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

beforeEach(() => {
  localStorage.clear();
  clearAcpProjectionRegistry();
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  clearAcpProjectionRegistry();
  compactViewport = false;
  useAgentArtifactAdmissionStore.getState().clear();
  useWorkbenchStore.setState(initialState, true);
});

describe("Agent center workbench window mode seam", () => {
  it("shows Chat in the Agent center without leaking an Editor-opened File into artifacts", async () => {
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
    expect(
      screen.getByRole("complementary", { name: "Agent artifacts" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: /current\.ts/ })).toBeNull();
    expect(activateTab).not.toHaveBeenCalled();
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

  it("keeps Chat in the Agent center when a Diff or File artifact tab is selected", async () => {
    seedArtifactOwner();
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
    useAgentArtifactAdmissionStore.getState().admit({
      owner: artifactOwner,
      tabId: "file:a",
    });
    useAgentArtifactAdmissionStore.getState().admit({
      owner: artifactOwner,
      tabId: "diff:d",
    });
    render(createElement(Workbench));

    const modeToggle = screen.getByRole("radiogroup", { name: "Window mode" });
    const agentControl = within(modeToggle).getByRole("radio", {
      name: "Agent",
    });
    fireEvent.click(agentControl);

    await vi.waitFor(() => {
      expect(screen.getByRole("region", { name: "Agent chat" }));
    });
    expect(activateTab).not.toHaveBeenCalled();

    const artifactStrip = within(
      screen.getByRole("navigation", { name: "Agent artifact tabs" }),
    );
    fireEvent.click(artifactStrip.getByRole("tab", { name: /changed\.ts/ }));

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenLastCalledWith("diff:d");
    });
    expect(agentControl).toBeChecked();
    expect(screen.getByRole("region", { name: "Agent chat" }));

    fireEvent.click(artifactStrip.getByRole("tab", { name: /current\.ts/ }));

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenLastCalledWith("file:a");
    });
    expect(activateTab).toHaveBeenCalledTimes(2);
    expect(agentControl).toBeChecked();
    expect(screen.getByText("Loading src/current.ts…")).toBeInTheDocument();
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
    expect(
      screen.getByRole("region", { name: "Agent chat" }),
    ).toBeInTheDocument();

    await act(async () => {
      useWorkbenchStore.setState({
        projectTransitioning: false,
        projectGeneration: initialState.projectGeneration + 2,
      });
    });

    await vi.waitFor(() => {
      expect(
        screen.getByRole("region", { name: "Agent chat" }),
      ).toBeInTheDocument();
    });
    expect(useWorkbenchStore.getState().activeTabId).toBe("file:b");
    expect(activateTab).not.toHaveBeenCalled();
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

  it("opens two Agent chats side by side", async () => {
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
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 1,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat:a",
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
        screen.getByRole("region", { name: "Chat pane A" }),
      ).toBeInTheDocument();
    });
    const paneA = screen.getByRole("region", { name: "Chat pane A" });
    const paneB = screen.getByRole("region", { name: "Chat pane B" });
    expect(
      within(paneA).getByRole("region", { name: "Agent chat" }),
    ).toBeInTheDocument();
    expect(
      within(paneB).getByRole("region", { name: "Agent chat" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/stays open beside the focused chat/i),
    ).toBeNull();
    expect(
      within(paneB)
        .getAllByRole("status")
        .map((node) => node.textContent),
    ).toEqual(["Idle"]);
    expect(screen.queryByText("Turn continues in the background.")).toBeNull();
  });

  it("keeps the Agent file open when toggling back to Editor", async () => {
    const activateTab = vi.fn(async (tabId: string) => {
      useWorkbenchStore.setState({ activeTabId: tabId });
    });
    useWorkbenchStore.setState({
      project,
      projects: [projectListItem],
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
      expect(screen.getByRole("region", { name: "Agent chat" }));
    });

    fireEvent.click(screen.getByTitle("Open chat B"));
    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenCalledWith("chat:b");
    });

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Editor" },
      ),
    );

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenLastCalledWith("file:a");
    });
    expect(screen.getByText("Loading src/current.ts…")).toBeInTheDocument();
  });

  it("does not steal an active File artifact after a transition flag reset", async () => {
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

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    await vi.waitFor(() => {
      expect(screen.getByRole("region", { name: "Agent chat" }));
    });
    expect(activateTab).not.toHaveBeenCalled();
    await pumpTransitionResets(3);
    expect(activateTab).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().activeTabId).toBe("file:a");
  });
});
