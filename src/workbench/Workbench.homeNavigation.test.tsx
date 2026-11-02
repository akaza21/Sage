import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { settleLazyModules } from "../test/settleLazyModules";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { Workbench } from "./Workbench";
import { LeftSidebar } from "./LeftSidebar";
import { AgentCollapsedNavigation } from "./AgentCollapsedNavigation";
import { acpDesktop } from "../services/acpDesktop";
import type { AcpChatPage } from "../domain/acp";
import { agentSessionFixture } from "../domain/acpProjection.fixture";
import { useSideChatState } from "./sideChatState";
import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { emptyChatOwnershipEntry } from "../app/chatOwnershipProjection";

const initialState = useWorkbenchStore.getState();
const ports = getWorkbenchPorts();

beforeEach(async () => {
  localStorage.clear();
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initialState, true);
  vi.stubGlobal("innerWidth", 1800);
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  await useWorkbenchStore.getState().initialize();
});

afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useSideChatState.setState({ sides: [], pairedId: null });
  useAgentSettingsState.getState().close();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  useWorkbenchStore.setState(initialState, true);
  resetBrowserDesktopForTests();
  localStorage.clear();
});

it("restores Files when split space returns and lets navigation reclaim the space", async () => {
  render(<Workbench />);
  await settleLazyModules();
  fireEvent.click(
    screen.getByRole("button", { name: /Show files and preview/ }),
  );
  await settleLazyModules();
  const composer = screen.getByRole("textbox", { name: "Message agent" });
  composer.focus();
  act(() => {
    vi.stubGlobal("innerWidth", 700);
    window.dispatchEvent(new Event("resize"));
  });
  expect(screen.queryByRole("dialog", { name: "Agent artifacts" })).toBeNull();
  expect(
    screen.queryByRole("complementary", { name: "Agent artifacts" }),
  ).toBeNull();
  expect(composer).toHaveFocus();
  act(() => {
    vi.stubGlobal("innerWidth", 800);
    window.dispatchEvent(new Event("resize"));
  });
  await settleLazyModules();
  expect(
    screen.getByRole("complementary", { name: "Agent artifacts" }),
  ).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "Show projects and chats" }),
  );
  await settleLazyModules();
  expect(
    screen.getByRole("button", { name: "Hide projects and chats" }),
  ).toBeVisible();
  expect(
    screen.queryByRole("complementary", { name: "Agent artifacts" }),
  ).toBeNull();
  expect(composer).toBeVisible();
});

it("keeps the visible chat title through resource activation and actual Agent Settings", async () => {
  await useWorkbenchStore.getState().createChat();
  useWorkbenchStore.setState((state) => ({
    tabs: state.tabs.map((tab, index) =>
      tab.kind === "chat"
        ? { ...tab, title: index === 0 ? "Chat: Earlier" : "Chat: Visible" }
        : tab,
    ),
  }));
  render(<Workbench />);
  await settleLazyModules();
  const heading = document.querySelector(".title-bar__session")!;
  expect(heading).toHaveTextContent("Visible");
  act(() =>
    useWorkbenchStore.setState((state) => ({
      tabs: [
        ...state.tabs,
        {
          id: "browser-title",
          title: "Page",
          kind: "browser",
          position: state.tabs.length,
          pinned: false,
          dirty: false,
          status: "closed",
        },
      ],
      activeTabId: "browser-title",
    })),
  );
  expect(heading).toHaveTextContent("Visible");
  act(() => useAgentSettingsState.getState().open());
  await settleLazyModules();
  expect(document.querySelector(".title-bar__session")).toHaveTextContent(
    "Visible",
  );
  act(() => useAgentSettingsState.getState().close());
  await settleLazyModules();
  expect(document.querySelector(".title-bar__session")).toHaveTextContent(
    "Visible",
  );
});

it.each([false, true])(
  "Home dismisses a nonmodal resource overlay before focusing Chat; collapsed=%s",
  async (collapsed) => {
    vi.stubGlobal("innerWidth", 700);
    const before = await ports.project.getBootstrapSnapshot();
    const create = vi.spyOn(ports.chat, "createChat");
    render(<Workbench />);
    await settleLazyModules();
    if (collapsed) {
      fireEvent.click(
        screen.getByRole("button", { name: "Hide projects and chats" }),
      );
      await settleLazyModules();
    }
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    fireEvent.change(composer, { target: { value: "Overlay draft" } });
    fireEvent.click(
      screen.getByRole("button", { name: /Show files and preview/ }),
    );
    const resources = screen.getByRole("dialog", { name: "Agent artifacts" });
    expect(resources).not.toHaveAttribute("aria-modal");
    expect(composer.closest(".center-region")).toHaveAttribute("inert");
    const home = screen.getByRole("button", { name: "Home" });
    expect(home.closest("aside")).not.toHaveAttribute("inert");
    home.focus();
    fireEvent.click(home);
    await settleLazyModules();
    expect(
      screen.queryByRole("dialog", { name: "Agent artifacts" }),
    ).toBeNull();
    expect(composer.closest(".center-region")).not.toHaveAttribute("inert");
    expect(composer).toHaveFocus();
    expect(composer).toHaveValue("Overlay draft");
    expect(await ports.project.getBootstrapSnapshot()).toEqual(before);
    expect(create).not.toHaveBeenCalled();
  },
);

it.each(["approval", "disabled"] as const)(
  "Home focuses the visible current Chat region when its composer is %s",
  async (unavailable) => {
    const before = await ports.project.getBootstrapSnapshot();
    const chatId = before.activeTabId!;
    const create = vi.spyOn(ports.chat, "createChat");
    useWorkbenchStore.setState({
      chatUiById: {
        [chatId]: {
          ...emptyChatOwnershipEntry(),
          composerText: "Preserved draft",
          sending: unavailable === "disabled",
        },
      },
    });
    if (unavailable === "disabled")
      useWorkbenchStore.setState((state) => ({
        harnesses: state.harnesses.map((harness) => ({
          ...harness,
          installState: "unavailable",
        })),
      }));
    if (unavailable === "approval") {
      const page = {
        projectId: before.project!.id,
        chatId,
        title: "Preserved Chat",
        projectionRevision: "1",
        currentSessionId: "session-a",
        sessions: [
          agentSessionFixture({
            bindingId: "codex-app-server" as never,
            runtimeEpoch: "1" as never,
          }),
        ],
        turns: [
          {
            id: "turn-home",
            sessionId: "session-a",
            runtimeEpoch: "1",
            ordinal: 0,
            state: "waitingApproval",
            revision: "1",
            createdAt: 1,
            updatedAt: 1,
            terminalAt: null,
            activities: [],
            browserOperations: [],
            browserOperationsCollapsed: 0,
            items: [
              {
                id: "permission-home",
                projectId: before.project!.id,
                chatId,
                sessionId: "session-a",
                turnId: "turn-home",
                runtimeEpoch: "1",
                ordinal: 0,
                origin: "primary",
                activityId: null,
                lifecycle: "streaming",
                revision: "1",
                createdAt: 1,
                updatedAt: 1,
                terminalAt: null,
                kind: "permissionRequest",
                toolState: "waitingForApproval",
                payload: {
                  requestId: "request-home",
                  state: "received",
                  label: "Permission request",
                  summary: "Run command",
                  offeredOptionKinds: ["allow_once", "reject_once"],
                  category: "execute",
                },
              },
            ],
          },
        ],
        nextCursor: null,
      } as unknown as AcpChatPage;
      vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue(page);
      useWorkbenchStore.setState({
        harnesses: [
          {
            id: "codex",
            name: "Codex",
            description: "ACP",
            transport: "acp",
            bindingId: "codex-app-server",
            installState: "available",
            source: "builtIn",
            accountModelDiscovery: true,
          },
        ],
        selectedHarnessId: "codex",
        chatUiById: {
          [chatId]: {
            ...emptyChatOwnershipEntry(),
            composerText: "Preserved draft",
            harnessId: "codex",
          },
        },
      });
    }
    render(<Workbench />);
    await settleLazyModules();
    if (unavailable === "approval") {
      expect(
        await screen.findByRole("group", { name: "Permission request" }),
      ).toBeVisible();
      expect(
        screen.queryByRole("textbox", { name: "Message agent" }),
      ).toBeNull();
    } else
      expect(
        screen.getByRole("textbox", { name: "Message agent" }),
      ).toBeDisabled();
    const pane = screen.getByRole("region", { name: /^Chat pane / });
    const home = screen.getByRole("button", { name: "Home" });
    home.focus();
    fireEvent.click(home);
    await settleLazyModules();
    expect(pane).toBeVisible();
    expect(pane).toHaveFocus();
    expect(pane).toHaveAttribute("tabindex", "-1");
    expect(useWorkbenchStore.getState().activeTabId).toBe(chatId);
    expect(useWorkbenchStore.getState().chatUiById[chatId]?.composerText).toBe(
      "Preserved draft",
    );
    expect(await ports.project.getBootstrapSnapshot()).toEqual(before);
    expect(create).not.toHaveBeenCalled();
  },
);

it.each(["parent", "side"] as const)(
  "Home keeps split ownership when the focused %s composer is unavailable",
  async (owner) => {
    const parentId = useWorkbenchStore.getState().activeTabId!;
    await useWorkbenchStore.getState().createChat();
    const sideId = useWorkbenchStore.getState().activeTabId!;
    const focusedId = owner === "parent" ? parentId : sideId;
    await useWorkbenchStore.getState().activateTab(focusedId);
    const before = await ports.project.getBootstrapSnapshot();
    const create = vi.spyOn(ports.chat, "createChat");
    useSideChatState.setState({
      sides: [
        { id: sideId, parentChatId: parentId, projectId: before.project!.id },
      ],
      pairedId: sideId,
    });
    useWorkbenchStore.setState((state) => ({
      harnesses: [
        ...state.harnesses,
        {
          ...state.harnesses[0]!,
          id: "fixture-unavailable",
          installState: "unavailable",
        },
      ],
      chatUiById: {
        [parentId]: {
          ...emptyChatOwnershipEntry(),
          composerText: "Parent draft",
          harnessId:
            owner === "parent"
              ? "fixture-unavailable"
              : state.selectedHarnessId,
        },
        [sideId]: {
          ...emptyChatOwnershipEntry(),
          composerText: "Side draft",
          harnessId:
            owner === "side" ? "fixture-unavailable" : state.selectedHarnessId,
        },
      },
    }));
    render(<Workbench />);
    await settleLazyModules();
    const panes = screen.getAllByRole("region", { name: /^Chat pane / });
    expect(panes).toHaveLength(2);
    const focused = panes.find((pane) => pane.dataset.chatId === focusedId)!;
    const other = panes.find((pane) => pane !== focused)!;
    expect(
      within(focused).getByRole("textbox", { name: "Message agent" }),
    ).toBeDisabled();
    expect(
      within(other).getByRole("textbox", { name: "Message agent" }),
    ).toBeEnabled();
    const home = screen.getByRole("button", { name: "Home" });
    home.focus();
    fireEvent.click(home);
    await settleLazyModules();
    expect(focused).toHaveFocus();
    expect(focused).toHaveClass("agent-chat-pane--focused");
    expect(useWorkbenchStore.getState().activeTabId).toBe(focusedId);
    expect(useSideChatState.getState().pairedId).toBe(sideId);
    expect(within(panes[0]!).getByRole("textbox")).toHaveValue("Parent draft");
    expect(within(panes[1]!).getByRole("textbox")).toHaveValue("Side draft");
    expect(await ports.project.getBootstrapSnapshot()).toEqual(before);
    expect(create).not.toHaveBeenCalled();
  },
);

it.each([false, true])(
  "Home never saves more chats with navigation collapsed=%s",
  async (collapsed) => {
    const before = await ports.project.getBootstrapSnapshot();
    expect(before.project).not.toBeNull();
    expect(before.tabs.filter((tab) => tab.kind === "chat")).toHaveLength(1);
    expect(before.messages.length).toBeGreaterThan(0);
    const create = vi.spyOn(ports.chat, "createChat");
    render(<Workbench />);
    await settleLazyModules();
    if (collapsed) {
      fireEvent.click(
        screen.getByRole("button", { name: "Hide projects and chats" }),
      );
      await settleLazyModules();
    }
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    fireEvent.change(composer, { target: { value: "Keep this unsent text" } });
    for (let click = 0; click < 3; click += 1) {
      const navigation = screen.getByRole("navigation", {
        name: "Primary navigation",
      });
      await act(async () =>
        fireEvent.click(within(navigation).getAllByRole("button")[0]!),
      );
      await settleLazyModules();
    }
    const after = await ports.project.getBootstrapSnapshot();
    expect(
      after.tabs.filter((tab) => tab.kind === "chat").map((tab) => tab.title),
    ).toEqual(
      before.tabs.filter((tab) => tab.kind === "chat").map((tab) => tab.title),
    );
    expect(after.archivedChats).toEqual(before.archivedChats);
    expect(create).not.toHaveBeenCalled();
    expect(after.activeTabId).toBe(before.activeTabId);
    expect(after.messages).toEqual(before.messages);
    expect(after.project).toEqual(before.project);
    expect(
      screen.getByRole("radio", { name: "Agent", checked: true }),
    ).toBeVisible();
    expect(screen.queryByText(/New chat [234]/)).toBeNull();
    expect(screen.getByRole("button", { name: "Home" })).toBeVisible();
    expect(composer).toHaveFocus();
    expect(composer).toHaveValue("Keep this unsent text");

    fireEvent.click(
      screen.getByRole("button", {
        name: collapsed ? "Open settings from rail" : "Open settings",
      }),
    );
    await settleLazyModules();
    fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
    await settleLazyModules();
    expect(composer).toHaveValue("Keep this unsent text");
    expect(useWorkbenchStore.getState().activeTabId).toBe(before.activeTabId);
    expect(create).not.toHaveBeenCalled();

    await act(async () =>
      fireEvent.click(
        screen.getByRole("button", {
          name: /^New chat$/,
        }),
      ),
    );
    await settleLazyModules();
    const created = await ports.project.getBootstrapSnapshot();
    expect(create).toHaveBeenCalledTimes(1);
    expect(created.tabs.filter((tab) => tab.kind === "chat")).toHaveLength(2);
    expect(created.activeTabId).not.toBe(before.activeTabId);
    expect(created.messages).toEqual([]);
    expect(created.project).toEqual(before.project);
    await act(async () =>
      useWorkbenchStore.getState().activateTab(before.activeTabId!),
    );
    const returned = await ports.project.getBootstrapSnapshot();
    expect(returned.messages).toEqual(before.messages);
    expect(returned.tabs.find((tab) => tab.id === before.activeTabId)).toEqual(
      before.tabs.find((tab) => tab.id === before.activeTabId),
    );
  },
);

it.each([false, true])(
  "Home remains navigation with no workspace; collapsed=%s",
  async (collapsed) => {
    useWorkbenchStore.setState({
      project: null,
      projects: [],
      tabs: [],
      activeTabId: null,
    });
    const create = vi.spyOn(ports.chat, "createChat");
    const ensureLocal = vi.spyOn(ports.project, "ensureLocalProject");
    render(
      collapsed ? (
        <AgentCollapsedNavigation
          inert={false}
          onOpenSettings={() => undefined}
          onExpand={() => undefined}
        />
      ) : (
        <LeftSidebar />
      ),
    );
    for (let click = 0; click < 3; click += 1)
      fireEvent.click(screen.getByRole("button", { name: "Home" }));
    await settleLazyModules();
    expect(create).not.toHaveBeenCalled();
    expect(ensureLocal).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().project).toBeNull();
    expect(useWorkbenchStore.getState().tabs).toEqual([]);
  },
);
