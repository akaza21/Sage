import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { useQuickOpenStore } from "../features/quickOpen/quickOpenState";
import { clearChatSearchFolderRequest } from "../features/chatSearch/chatSearchScope";
import { LeftSidebar } from "./LeftSidebar";
import { TitleBar } from "./TitleBar";
import { preloadSearchablePicker } from "../ui/searchablePickerLoad";
import { useSideChatState } from "./sideChatState";
import { resetAgentChatHistory } from "./agentChatHistory";

// The picker list loads when the window first idles.
beforeAll(() => preloadSearchablePicker());

const initialState = useWorkbenchStore.getState();

it("keeps brand artwork and the wordmark out of the title bar", () => {
  const { container } = render(
    <TitleBar mode="agent" onModeChange={vi.fn()} />,
  );
  expect(
    container.querySelector(".title-bar__mark, .title-bar__brand"),
  ).toBeNull();
  expect(screen.queryByText("Sage")).toBeNull();
});

afterEach(() => {
  cleanup();
  resetAgentChatHistory();
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
    render(<TitleBar mode="editor" onModeChange={onModeChange} />);

    const modes = screen.getByRole("radiogroup", { name: "Window mode" });
    const agent = within(modes).getByRole("radio", { name: "Agent" });
    const editor = within(modes).getByRole("radio", { name: "Editor" });
    expect(editor).toBeChecked();
    expect(agent).not.toBeChecked();

    editor.focus();
    fireEvent.keyDown(editor, { key: "ArrowRight" });
    fireEvent.click(agent);
    expect(onModeChange).toHaveBeenCalledWith("agent");
  });

  it("anchors the mode chooser in the title bar for every mode and layout", () => {
    seedChats();
    const { rerender } = render(
      <TitleBar mode="agent" onModeChange={vi.fn()} />,
    );
    const start = () => document.querySelector(".title-bar__start");

    for (const mode of ["agent", "editor", "agent"] as const) {
      rerender(<TitleBar mode={mode} onModeChange={vi.fn()} />);
      const modes = screen.getByRole("radiogroup", { name: "Window mode" });
      expect(start()).toContainElement(modes);
      expect(screen.queryByText("Sage")).toBeNull();
    }

    rerender(<TitleBar mode="agent" onModeChange={vi.fn()} />);
    expect(
      within(screen.getByRole("banner")).getByRole("button", {
        name: "Search chats",
      }),
    ).toBeEnabled();
  });

  it("shows Back and Forward in Agent, Quick Open's Search files in Editor, and no chat title (Codex)", () => {
    seedChats();
    const { rerender } = render(
      <TitleBar mode="agent" onModeChange={vi.fn()} />,
    );
    const banner = screen.getByRole("banner");
    expect(within(banner).queryByText("Review")).toBeNull();
    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Search files" })).toBeNull();

    rerender(<TitleBar mode="editor" onModeChange={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Search files" }));
    expect(useQuickOpenStore.getState().open).toBe(true);
    useQuickOpenStore.setState({ open: false });
    expect(
      within(screen.getByRole("banner")).queryByRole("button", {
        name: "Search chats",
      }),
    ).toBeNull();
  });

  it("offers Back once another chat is shown, and a side chat counts as its parent", () => {
    seedChats();
    const { tabs } = useWorkbenchStore.getState();
    useWorkbenchStore.setState({
      tabs: [
        ...tabs,
        {
          id: "side-1",
          kind: "chat",
          title: "Side chat",
          position: 2,
          pinned: false,
          dirty: false,
        },
        {
          id: "chat:c",
          kind: "chat",
          title: "Chat: Other",
          position: 3,
          pinned: false,
          dirty: false,
        },
      ],
    });
    useSideChatState.setState({
      sides: [{ id: "side-1", projectId: "project-a", parentChatId: "chat:b" }],
    });
    try {
      render(<TitleBar mode="agent" onModeChange={vi.fn()} />);
      const back = screen.getByRole("button", { name: "Back" });
      act(() => useWorkbenchStore.setState({ activeTabId: "side-1" }));
      expect(back).toBeDisabled();
      act(() => useWorkbenchStore.setState({ activeTabId: "chat:c" }));
      expect(back).toBeEnabled();
      expect(back).toHaveAttribute("title", "Back (Alt+Left)");
    } finally {
      useSideChatState.setState({ sides: [] });
    }
  });

  it("opens Chat search from the search chord and shows it on the control", async () => {
    seedChats();
    render(
      <>
        <TitleBar mode="agent" onModeChange={vi.fn()} />
        <LeftSidebar />
      </>,
    );
    const titleSearch = within(screen.getByRole("banner")).getByRole("button", {
      name: "Search chats",
    });
    expect(titleSearch).toHaveTextContent("Ctrl+K");
    expect(titleSearch).toHaveAttribute("aria-keyshortcuts", "Control+K");
    expect(screen.queryByRole("dialog", { name: "Search chats" })).toBeNull();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    expect(
      await screen.findByRole("dialog", { name: "Search chats" }),
    ).toBeTruthy();
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
        <LeftSidebar />
      </>,
    );
    const titleSearch = within(screen.getByRole("banner")).getByRole("button", {
      name: "Search chats",
    });
    expect(titleSearch).toBeEnabled();
    fireEvent.click(titleSearch);
    const dialog = screen.getByRole("dialog", { name: "Search chats" });
    expect(screen.getByRole("button", { name: "Search in" })).toHaveTextContent(
      "Workspace",
    );
    expect(useWorkbenchStore.getState().sidebarView).toBe("explorer");
    expect(useWorkbenchStore.getState().sidebarOpen).toBe(false);
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(titleSearch).toHaveFocus());
  });

  it("opens one Chat search from the titlebar with persistent Agent navigation", async () => {
    seedChats();
    render(
      <>
        <TitleBar mode="agent" onModeChange={vi.fn()} />
        <LeftSidebar />
      </>,
    );
    const titleSearch = within(screen.getByRole("banner")).getByRole("button", {
      name: "Search chats",
    });
    expect(
      screen.getByRole("complementary", { name: "Projects and chats" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Open projects and chats" }),
    ).toBeNull();
    expect(screen.getByRole("button", { name: "New chat" })).toBeVisible();
    titleSearch.focus();
    fireEvent.click(titleSearch);
    const dialog = screen.getByRole("dialog", { name: "Search chats" });
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    fireEvent.click(within(dialog).getByRole("button", { name: "Search in" }));
    const folderInput = within(dialog).getByRole("combobox", {
      name: "Search in",
    });
    fireEvent.keyDown(folderInput, { key: "Home" });
    fireEvent.keyDown(folderInput, { key: "Enter" });
    expect(
      within(dialog).getByRole("button", { name: "Search in" }),
    ).toHaveTextContent("All folders");
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    await waitFor(() => expect(titleSearch).toHaveFocus());
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: "New chat" })).toBeVisible();
    expect(useWorkbenchStore.getState().sidebarOpen).toBe(false);
  });

  it("consumes a titlebar request queued before the lazy Agent body mounts", async () => {
    seedChats();
    const { rerender } = render(
      <TitleBar mode="agent" onModeChange={vi.fn()} />,
    );
    const titleSearch = within(screen.getByRole("banner")).getByRole("button", {
      name: "Search chats",
    });
    titleSearch.focus();
    fireEvent.click(titleSearch);
    expect(screen.queryByRole("dialog")).toBeNull();
    rerender(
      <>
        <TitleBar mode="agent" onModeChange={vi.fn()} />
        <LeftSidebar />
      </>,
    );
    const dialog = screen.getByRole("dialog", { name: "Search chats" });
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    await waitFor(() =>
      expect(
        within(screen.getByRole("banner")).getByRole("button", {
          name: "Search chats",
        }),
      ).toHaveFocus(),
    );
  });

  it("retains one search owner as persistent navigation resizes", () => {
    seedChats();
    const view = (width: number) => (
      <div style={{ width }}>
        <TitleBar mode="agent" onModeChange={vi.fn()} />
        <LeftSidebar />
      </div>
    );
    const { rerender } = render(view(480));
    fireEvent.click(
      within(screen.getByRole("banner")).getByRole("button", {
        name: "Search chats",
      }),
    );
    const dialog = screen.getByRole("dialog", { name: "Search chats" });
    for (const width of [720, 1280, 480]) {
      rerender(view(width));
      expect(screen.getAllByRole("dialog", { name: "Search chats" })).toEqual([
        dialog,
      ]);
    }
  });
});
