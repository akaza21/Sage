import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { settleLazyModules } from "../test/settleLazyModules";
import { Workbench } from "./Workbench";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { clearChatSearchFolderRequest } from "../features/chatSearch/chatSearchScope";

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () => <textarea aria-label="Composer" />,
}));

const initialState = useWorkbenchStore.getState();
const tabs = ["a", "b"].map((name, position) => ({
  id: `chat:${name}`,
  kind: "chat" as const,
  title: `Chat ${name}`,
  position,
  pinned: false,
  dirty: false,
}));
let resizeCenter: (width: number) => void;

beforeEach(() => {
  resizeCenter = () => undefined;
  localStorage.clear();
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
      constructor(private callback: ResizeObserverCallback) {}
      observe(target: Element) {
        if (target.id === "agent-chat-panel")
          resizeCenter = (width) =>
            act(() =>
              this.callback(
                [{ contentRect: { width } } as ResizeObserverEntry],
                {} as ResizeObserver,
              ),
            );
      }
      unobserve() {}
      disconnect() {}
    },
  );
  useWorkbenchStore.setState({
    ...initialState,
    phase: "ready",
    project: { id: "project-a", name: "A", roots: [] },
    projects: [{ id: "project-a", name: "A", rootCount: 0, lastOpenedAt: 1 }],
    tabs,
    activeTabId: "chat:b",
    sidebarOpen: true,
    activateTab: vi.fn(async (activeTabId) =>
      useWorkbenchStore.setState({ activeTabId }),
    ),
    createChat: vi.fn(async () =>
      useWorkbenchStore.setState({
        tabs: [...tabs, { ...tabs[0]!, id: "chat:new", title: "New chat" }],
        activeTabId: "chat:new",
      }),
    ),
  });
});

afterEach(() => {
  cleanup();
  useAgentSettingsState.getState().close();
  clearChatSearchFolderRequest();
  localStorage.clear();
  vi.unstubAllGlobals();
  useWorkbenchStore.setState(initialState, true);
});

it("toggles Agent navigation with Ctrl+B without creating a Chat", async () => {
  const { container } = render(<Workbench />);
  await settleLazyModules();
  fireEvent.keyDown(window, { key: "b", ctrlKey: true });
  await settleLazyModules();
  expect(container.querySelector(".workbench")).toHaveClass(
    "workbench--navigation-collapsed",
  );
  fireEvent.keyDown(window, { key: "b", ctrlKey: true });
  await settleLazyModules();
  expect(container.querySelector(".workbench")).not.toHaveClass(
    "workbench--navigation-collapsed",
  );
  expect(useWorkbenchStore.getState().createChat).not.toHaveBeenCalled();
});

it("collapses on entering a narrow breakpoint after desktop sidebar interaction", async () => {
  let update = () => {};
  const media = {
    matches: false,
    addEventListener: (_: string, listener: () => void) => {
      update = listener;
    },
    removeEventListener: vi.fn(),
  };
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) =>
      query === "(max-width: 981px)"
        ? media
        : {
            matches: false,
            addEventListener: vi.fn(),
            removeEventListener: vi.fn(),
          },
    ),
  );
  const { container } = render(<Workbench />);
  await settleLazyModules();
  fireEvent.keyDown(window, { key: "b", ctrlKey: true });
  fireEvent.keyDown(window, { key: "b", ctrlKey: true });
  expect(container.querySelector(".workbench")).not.toHaveClass(
    "workbench--navigation-collapsed",
  );
  act(() => {
    media.matches = true;
    update();
  });
  await settleLazyModules();
  expect(container.querySelector(".workbench")).toHaveClass(
    "workbench--navigation-collapsed",
  );
});

it.each(["button", "shortcut"])(
  "opens visible Chat search from Agent Settings using the %s",
  async (trigger) => {
    render(<Workbench />);
    await settleLazyModules();
    act(() => useAgentSettingsState.getState().open());
    expect(
      await screen.findByRole("dialog", { name: "Agent settings" }),
    ).toBeVisible();
    expect(screen.queryByRole("textbox", { name: "Composer" })).toBeNull();
    if (trigger === "button")
      fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    else fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(
      await screen.findByRole("dialog", { name: "Search Chats" }),
    ).toBeVisible();
    expect(useAgentSettingsState.getState().page).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Agent settings" })).toBeNull();
  },
);

it("starts a fresh profile in Agent, and never adds a chat when resources collapse or the center crosses 860px", async () => {
  const { container } = render(<Workbench />);
  await settleLazyModules();
  expect(
    screen.getByRole("radio", { name: "Agent", checked: true }),
  ).toBeInTheDocument();
  const visibleChats = () =>
    [...container.querySelectorAll(".agent-chat-pane")].map((pane) =>
      pane.getAttribute("data-chat-id"),
    );
  for (const width of [859, 860, 1200]) {
    resizeCenter(width);
    expect(visibleChats()).toEqual(["chat:b"]);
  }
  fireEvent.click(
    screen.getByRole("button", { name: /Show files and preview/ }),
  );
  resizeCenter(700);
  fireEvent.click(
    screen.getByRole("button", {
      name: "Collapse files, changes, and preview",
    }),
  );
  resizeCenter(1200);
  expect(visibleChats()).toEqual(["chat:b"]);
  fireEvent.click(screen.getByRole("button", { name: "New chat" }));
  expect(useWorkbenchStore.getState().createChat).toHaveBeenCalledTimes(1);
  expect(visibleChats()).toEqual(["chat:new"]);
});

it("collapses navigation to its icon rail, keeps header New chat usable, and restores the user's divider width", async () => {
  const { container } = render(<Workbench />);
  await settleLazyModules();
  const splitter = screen.getByRole("separator", {
    name: "Resize project and chat rail",
  });
  fireEvent.keyDown(splitter, { key: "End" });
  const body = container.querySelector(".workbench__body") as HTMLElement;
  const columns = body.style.gridTemplateColumns;
  expect(columns).toMatch(/^460px /);
  fireEvent.click(
    screen.getByRole("button", { name: "Hide projects and chats" }),
  );
  expect(
    screen.getByRole("button", { name: "Show projects and chats" }),
  ).toHaveAttribute("aria-expanded", "false");
  await settleLazyModules();
  expect(
    container.querySelector(".project-sidebar__icon-rail"),
  ).toBeInTheDocument();
  expect(container.querySelector(".workbench")).toHaveClass(
    "workbench--navigation-collapsed",
  );
  expect(container.querySelector(".project-sidebar__scroll")).toBeNull();
  expect(container.querySelector(".project-sidebar__actions")).toBeNull();
  expect(container.querySelector(".project-sidebar__account")).toBeNull();
  expect(body.style.gridTemplateColumns).toBe("46px minmax(0, 1fr)");
  expect(
    screen.queryByRole("separator", { name: "Resize project and chat rail" }),
  ).toBeNull();
  const newChat = container.querySelector<HTMLButtonElement>(
    ".agent-task-header button[aria-label='New chat']",
  )!;
  fireEvent.click(newChat);
  expect(useWorkbenchStore.getState().createChat).toHaveBeenCalledTimes(1);
  fireEvent.click(
    screen.getByRole("button", { name: "Show projects and chats" }),
  );
  expect(body.style.gridTemplateColumns).toBe(columns);
  expect(
    container.querySelector(".agent-task-header button[aria-label='New chat']"),
  ).toBeNull();
});
