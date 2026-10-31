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
import { useQuickOpenStore } from "../features/quickOpen/quickOpenState";
import { clearChatSearchFolderRequest } from "../features/chatSearch/chatSearchScope";
import { LeftSidebar } from "./LeftSidebar";
import { TitleBar } from "./TitleBar";
import { useSideChatState } from "./sideChatState";

const initialState = useWorkbenchStore.getState();

it("draws the Sage leaf mark as an inline shape that inherits the neutral text colour", () => {
  const { container } = render(
    <TitleBar mode="agent" onModeChange={vi.fn()} />,
  );
  const mark = container.querySelector("svg.title-bar__mark");
  expect(mark).not.toBeNull();
  expect(mark!.getAttribute("fill")).toBe("currentColor");
  expect(mark!.getAttribute("aria-hidden")).toBe("true");
  expect(mark!.querySelectorAll("path").length).toBe(2);
  expect(mark!.outerHTML).not.toMatch(/#[0-9a-fA-F]{3,8}|rgb|green/);
  expect(screen.getByText("Sage")).toBeTruthy();
  expect(container.querySelector(".title-bar__logo")).toBeNull();
});

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
      expect(screen.getByText("Sage")).toBeTruthy();
    }

    rerender(<TitleBar mode="agent" onModeChange={vi.fn()} />);
    expect(
      within(screen.getByRole("banner")).getByRole("button", {
        name: "Search chats",
      }),
    ).toBeEnabled();
  });

  it("shows the focused session title in Agent and Quick Open's Search files in Editor", () => {
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
    expect(useQuickOpenStore.getState().open).toBe(true);
    useQuickOpenStore.setState({ open: false });
    expect(
      within(screen.getByRole("banner")).queryByRole("button", {
        name: "Search chats",
      }),
    ).toBeNull();
  });

  it("keeps the parent's window title while its side chat has focus (Codex s2/f0244)", () => {
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
      ],
      activeTabId: "side-1",
    });
    useSideChatState.setState({
      sides: [{ id: "side-1", projectId: "project-a", parentChatId: "chat:b" }],
    });
    try {
      render(<TitleBar mode="agent" onModeChange={vi.fn()} />);
      expect(screen.getByRole("heading", { name: "Review" })).toBeVisible();
      expect(screen.queryByRole("heading", { name: "Side chat" })).toBeNull();
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
    expect(screen.queryByRole("dialog", { name: "Search Chats" })).toBeNull();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    expect(
      await screen.findByRole("dialog", { name: "Search Chats" }),
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
    const dialog = screen.getByRole("dialog", { name: "Search Chats" });
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
    const dialog = screen.getByRole("dialog", { name: "Search Chats" });
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
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
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
    const dialog = screen.getByRole("dialog", { name: "Search Chats" });
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
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
    const dialog = screen.getByRole("dialog", { name: "Search Chats" });
    for (const width of [720, 1280, 480]) {
      rerender(view(width));
      expect(screen.getAllByRole("dialog", { name: "Search Chats" })).toEqual([
        dialog,
      ]);
    }
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
