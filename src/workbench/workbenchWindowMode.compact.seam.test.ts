import { createElement } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import {
  beforeEach,
  afterEach,
  describe,
  expect,
  it,
  onTestFinished,
  vi,
} from "vitest";

import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import type { AcpChatPage, ProjectGeneration } from "../domain/acp";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { settleLazyModules } from "../test/settleLazyModules";
import { Workbench } from "./Workbench";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () =>
    createElement("section", { "aria-label": "Agent chat" }, "Active chat"),
}));

let compactViewport = false;

vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: (query?: string) => !query && compactViewport,
}));

const initialState = useWorkbenchStore.getState();
const project = { id: "project-a", name: "A", roots: [] };
const artifactOwner = {
  projectId: "project-a",
  chatId: "chat:b",
  sessionId: "session-b",
} as AgentArtifactOwner;

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

beforeEach(() => writeWorkbenchLayout({ windowMode: "editor" }));

afterEach(() => {
  cleanup();
  localStorage.clear();
  compactViewport = false;
  clearAcpProjectionRegistry();
  useAgentArtifactAdmissionStore.getState().clear();
  useWorkbenchStore.setState(initialState, true);
});

async function switchWindowMode(mode: "Agent" | "Editor") {
  fireEvent.click(
    within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
      "radio",
      { name: mode },
    ),
  );
  await settleLazyModules();
}

describe("compact workbench window mode seam", () => {
  it("keeps the center interactive when Agent mode replaces an open compact Editor drawer", async () => {
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
    await settleLazyModules();
    const center = container.querySelector(".center-region");
    expect(center).toHaveAttribute("inert");
    expect(center).toHaveAttribute("aria-hidden", "true");
    expect(
      screen.getByRole("dialog", { name: "Explorer project tools" }),
    ).toBeInTheDocument();

    await switchWindowMode("Agent");

    expect(screen.queryByRole("dialog")).toBeNull();
    await screen.findByRole("button", {
      name: "Show files and preview · Waiting",
    });
    const agentCenter = container.querySelector(".center-region");
    expect(agentCenter).not.toHaveAttribute("inert");
    expect(agentCenter).not.toHaveAttribute("aria-hidden");

    await switchWindowMode("Editor");

    expect(
      screen.getByRole("dialog", { name: "Explorer project tools" }),
    ).toBeInTheDocument();
    const editorCenter = [...container.querySelectorAll(".center-region")].find(
      (region) => !region.closest("[style*='display: none']"),
    );
    expect(editorCenter).toHaveAttribute("inert");
    expect(editorCenter).toHaveAttribute("aria-hidden", "true");
  });

  it("opens new Agent resources on the right while tabs change", async () => {
    compactViewport = true;
    // Narrow enough that resources open over Chat rather than beside it.
    const originalWidth = window.innerWidth;
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 960,
    });
    onTestFinished(() => {
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: originalWidth,
      });
    });
    seedArtifactOwner();
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
    useAgentArtifactAdmissionStore.getState().admit({
      owner: artifactOwner,
      tabId: "file:a",
    });
    const { container } = render(createElement(Workbench));
    await settleLazyModules();
    const center = container.querySelector(".center-region");
    expect(center).toHaveAttribute("inert");

    await switchWindowMode("Agent");

    await vi.waitFor(() => {
      expect(
        screen.getByRole("region", { name: "Agent chat", hidden: true }),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByRole("dialog", { name: "Agent artifacts" }),
    ).toBeVisible();
    const agentCenter = container.querySelector(".center-region");
    expect(agentCenter).toHaveAttribute("inert");
    expect(agentCenter).toHaveAttribute("aria-hidden", "true");

    fireEvent.click(
      within(
        screen.getByRole("navigation", { name: "Agent artifact tabs" }),
      ).getByRole("tab", { name: /current\.ts/ }),
    );
    await settleLazyModules();

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenLastCalledWith("file:a");
    });
    expect(agentCenter).toHaveAttribute("inert");
    expect(agentCenter).toHaveAttribute("aria-hidden", "true");
    expect(
      screen.getByRole("region", { name: "Agent chat", hidden: true }),
    ).toBeInTheDocument();
  });
});
