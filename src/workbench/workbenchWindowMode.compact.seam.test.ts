import { createElement } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
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

afterEach(() => {
  cleanup();
  compactViewport = false;
  useAgentArtifactAdmissionStore.getState().clear();
  useWorkbenchStore.setState(initialState, true);
});

describe("compact workbench window mode seam", () => {
  it("keeps the center interactive when Agent mode replaces an open compact Editor drawer", () => {
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
    const center = container.querySelector(".center-region");
    expect(center).toHaveAttribute("inert");
    expect(center).toHaveAttribute("aria-hidden", "true");
    expect(
      screen.getByRole("dialog", { name: "Explorer project tools" }),
    ).toBeInTheDocument();

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(center).not.toHaveAttribute("inert");
    expect(center).not.toHaveAttribute("aria-hidden");

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Editor" },
      ),
    );

    expect(
      screen.getByRole("dialog", { name: "Explorer project tools" }),
    ).toBeInTheDocument();
    expect(center).toHaveAttribute("inert");
    expect(center).toHaveAttribute("aria-hidden", "true");
  });

  it("opens new Agent resources on the right while tabs change", async () => {
    compactViewport = true;
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
    useAgentArtifactAdmissionStore.getState().admit("file:a");
    const { container } = render(createElement(Workbench));
    const center = container.querySelector(".center-region");
    expect(center).toHaveAttribute("inert");

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );

    await vi.waitFor(() => {
      expect(
        screen.getByRole("region", { name: "Agent chat", hidden: true }),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByRole("complementary", { name: "Agent artifacts" }),
    ).not.toHaveClass("agent-artifact-pane--collapsed");
    expect(center).toHaveAttribute("inert");
    expect(center).toHaveAttribute("aria-hidden", "true");

    fireEvent.click(
      within(
        screen.getByRole("navigation", { name: "Agent artifact tabs" }),
      ).getByRole("tab", { name: /current\.ts/ }),
    );

    await vi.waitFor(() => {
      expect(activateTab).toHaveBeenLastCalledWith("file:a");
    });
    expect(center).toHaveAttribute("inert");
    expect(center).toHaveAttribute("aria-hidden", "true");
    expect(
      screen.getByRole("region", { name: "Agent chat", hidden: true }),
    ).toBeInTheDocument();
  });
});
