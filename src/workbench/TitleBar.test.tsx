import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { ChatSearchControl } from "../features/chatSearch/ChatSearchControl";
import { clearChatSearchFolderRequest } from "../features/chatSearch/chatSearchScope";
import { TitleBar } from "./TitleBar";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  clearChatSearchFolderRequest();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

function seedChats() {
  useWorkbenchStore.setState({
    phase: "ready",
    projectGeneration: 3,
    nativeTransitionEpoch: 2,
    projectTransitioning: false,
    projects: [
      { id: "project-a", name: "Workspace", rootCount: 1, lastOpenedAt: 1 },
    ],
    project: { id: "project-a", name: "Workspace", roots: [] },
    tabs: [
      {
        id: "chat:b",
        kind: "chat",
        title: "Chat: Review",
        position: 0,
        pinned: false,
        dirty: false,
      },
      {
        id: "file:app",
        kind: "file",
        title: "app.ts",
        position: 1,
        pinned: false,
        dirty: false,
        resource: { rootId: "root-a", relativePath: "src/app.ts" },
      },
    ],
    activeTabId: "chat:b",
    sidebarOpen: false,
    sidebarView: "explorer",
  });
}

describe("TitleBar center chrome", () => {
  it("keeps the mode chooser and Agent chat search named and keyboard reachable", () => {
    seedChats();
    const onModeChange = vi.fn();
    render(<TitleBar mode="agent" onModeChange={onModeChange} />);

    const modes = screen.getByRole("radiogroup", { name: "Window mode" });
    const agent = within(modes).getByRole("radio", { name: "Agent" });
    const editor = within(modes).getByRole("radio", { name: "Editor" });
    const chatSearch = screen.getByRole("button", { name: "Search chats" });
    expect(agent).toBeChecked();
    expect(editor).not.toBeChecked();
    expect(chatSearch).toBeEnabled();

    agent.focus();
    fireEvent.keyDown(agent, { key: "ArrowRight" });
    fireEvent.click(editor);
    expect(onModeChange).toHaveBeenCalledWith("editor");
  });

  it("shows the focused session title in Agent and Search files in Editor", () => {
    seedChats();
    const { rerender } = render(
      <TitleBar mode="agent" onModeChange={vi.fn()} />,
    );

    expect(screen.getByRole("heading", { name: "Review" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Search files" })).toBeNull();

    useWorkbenchStore.setState({ activeTabId: "file:app" });
    rerender(<TitleBar mode="agent" onModeChange={vi.fn()} />);
    expect(screen.getByRole("heading", { name: "Review" })).toBeVisible();

    rerender(<TitleBar mode="editor" onModeChange={vi.fn()} />);
    expect(screen.queryByRole("heading", { name: "Review" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Search files" }));
    expect(useWorkbenchStore.getState().sidebarView).toBe("search");
    expect(useWorkbenchStore.getState().sidebarOpen).toBe(true);
    expect(
      within(screen.getByRole("banner")).queryByRole("button", {
        name: "Search chats",
      }),
    ).toBeNull();
  });

  it("opens the durable Settings tab from Editor chrome", async () => {
    seedChats();
    render(<TitleBar mode="editor" onModeChange={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Open settings" }));

    await vi.waitFor(() => {
      expect(useWorkbenchStore.getState().activeTabId).toBe(
        "settings:appearance",
      );
    });
    expect(useWorkbenchStore.getState().tabs).toContainEqual(
      expect.objectContaining({ kind: "settings", title: "Settings" }),
    );
  });

  it("opens Chat search from Agent without switching to Search files", async () => {
    seedChats();
    render(
      <>
        <TitleBar mode="agent" onModeChange={vi.fn()} />
        <ChatSearchControl />
      </>,
    );
    const titleSearch = within(screen.getByRole("banner")).getByRole("button", {
      name: "Search chats",
    });
    expect(titleSearch).toBeEnabled();
    fireEvent.click(titleSearch);
    const dialog = screen.getByRole("dialog", { name: "Search Chats" });
    expect(screen.getByRole("button", { name: "Search in" })).toHaveTextContent(
      "Workspace",
    );
    expect(useWorkbenchStore.getState().sidebarView).toBe("explorer");
    expect(useWorkbenchStore.getState().sidebarOpen).toBe(false);
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(titleSearch).toHaveFocus());
  });

  it("hides an invalid session title instead of painting it", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat:bad",
          kind: "chat",
          title: "Chat: bad\ntitle",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat:bad",
    });
    render(<TitleBar mode="agent" onModeChange={vi.fn()} />);
    expect(screen.queryByRole("heading")).toBeNull();
  });
});
