import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { Workbench } from "./Workbench";

const initialState = useWorkbenchStore.getState();
let compactViewport = true;
let setCompactViewport: (compact: boolean) => void;

beforeEach(() => {
  Object.defineProperty(Element.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
  compactViewport = true;
  const listeners = new Set<() => void>();
  setCompactViewport = (compact) => {
    compactViewport = compact;
    listeners.forEach((listener) => listener());
  };
  vi.stubGlobal(
    "matchMedia",
    vi.fn().mockImplementation((query: string) => ({
      get matches() {
        return compactViewport && query === "(max-width: 760px)";
      },
      media: query,
      onchange: null,
      addEventListener: (_type: string, listener: () => void) =>
        listeners.add(listener),
      removeEventListener: (_type: string, listener: () => void) =>
        listeners.delete(listener),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  );
  useWorkbenchStore.setState({
    projects: [],
    project: null,
    tabs: [],
    activeTabId: null,
    sidebarOpen: false,
    sidebarView: "explorer",
    repositories: { roots: [], truncated: false, elapsedMs: 0 },
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  useWorkbenchStore.setState(initialState, true);
});

describe("compact Workbench project tools", () => {
  it("routes Chat navigation from the sidebar through authoritative activation", () => {
    const activateTab = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      activateTab,
      project: { id: "project-a", name: "A", roots: [] },
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "Chat: A",
          position: 0,
          pinned: true,
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
    render(<Workbench />);

    fireEvent.click(screen.getByTitle("Open chat B"));

    expect(activateTab).toHaveBeenCalledWith("chat:b");
  });

  it("routes both New Chat controls through one creation action", () => {
    const createChat = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      createChat,
      project: { id: "project-a", name: "A", roots: [] },
    });
    render(<Workbench />);

    fireEvent.click(screen.getByRole("button", { name: "New chat" }));
    fireEvent.click(screen.getByRole("button", { name: "New tab" }));

    expect(createChat).toHaveBeenCalledTimes(2);
  });

  it("uses one accessible drawer and restores its rail focus after Escape", () => {
    const { container } = render(<Workbench />);
    const center = container.querySelector(".center-region");
    const sourceControl = screen.getByRole("button", {
      name: "Source Control",
    });

    expect(sourceControl).toHaveAttribute(
      "aria-controls",
      "project-tools-panel",
    );
    expect(sourceControl).toHaveAttribute("aria-expanded", "false");
    expect(center).not.toHaveAttribute("inert");

    fireEvent.click(sourceControl);
    const drawer = screen.getByRole("dialog", {
      name: "Source Control project tools",
    });
    expect(drawer).toHaveAttribute("id", "project-tools-panel");
    expect(drawer).toHaveFocus();
    expect(sourceControl).toHaveAttribute("aria-expanded", "true");
    expect(center).toHaveAttribute("inert");
    expect(center).toHaveAttribute("aria-hidden", "true");

    fireEvent.keyDown(drawer, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(sourceControl).toHaveFocus();
    expect(sourceControl).toHaveAttribute("aria-expanded", "false");
    expect(center).not.toHaveAttribute("inert");
    expect(center).not.toHaveAttribute("aria-hidden");
  });

  it("moves focus out of the center on a compact transition", () => {
    compactViewport = false;
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "sourceControl",
    });
    const { container } = render(<Workbench />);
    const center = container.querySelector(".center-region");
    const centerButton = screen.getByRole("button", { name: "New tab" });
    centerButton.focus();

    act(() => setCompactViewport(true));
    const drawer = screen.getByRole("dialog", {
      name: "Source Control project tools",
    });
    expect(drawer).toHaveFocus();
    expect(center).toHaveAttribute("inert");

    const drawerButton = within(drawer).getByRole("button", {
      name: "Refresh Source Control",
    });
    drawerButton.focus();
    act(() => setCompactViewport(false));
    act(() => setCompactViewport(true));
    expect(drawerButton).toHaveFocus();
  });
});
