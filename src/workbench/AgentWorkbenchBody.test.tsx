import { useState } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { clearChatSearchFolderRequest } from "../features/chatSearch/chatSearchScope";
import { AgentWorkbenchBody } from "./AgentWorkbenchBody";
import { agentWorkbenchLayout } from "./agentWorkbenchLayout";
import {
  resetAgentRailViewPrefs,
  setAgentRailGrouping,
} from "./agentRailViewPrefs";

vi.mock("./AgentChatPanes", () => ({ AgentChatPanes: () => null }));
vi.mock("./AgentArtifactPane", () => ({ AgentArtifactPane: () => null }));

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  clearChatSearchFolderRequest();
  resetAgentRailViewPrefs();
  useWorkbenchStore.setState(initial, true);
});

function NavigationWorkbench() {
  const [collapsed, setCollapsed] = useState(false);
  return (
    <>
      <button onClick={() => setCollapsed((value) => !value)}>
        Toggle navigation
      </button>
      <AgentWorkbenchBody
        layout={agentWorkbenchLayout({
          viewportWidth: 1888,
          artifactExpanded: false,
          navigationCollapsed: collapsed,
        })}
        style={undefined}
        drawerOpen={false}
        navigationCollapsed={collapsed}
        onExpandNavigation={() => setCollapsed(false)}
        artifactExpanded={false}
        onArtifactExpandedChange={() => undefined}
        onAgentLeftRailWidthChange={() => undefined}
        onAgentArtifactWidthChange={() => undefined}
        onOpenArtifactInEditor={() => undefined}
        onOpenSettings={() => undefined}
      />
    </>
  );
}

it.each([
  ["Go to recent chats", "Recent", "folder", false],
  ["Go to projects", "Projects", "folder", false],
  ["Go to recent chats", "Recent", "folder", true],
  ["Go to projects", "Projects", "folder", true],
  ["Go to projects", "Chats", "status", false],
  ["Go to projects", "Chats", "status", true],
] as const)(
  "%s reveals %s in %s grouping (section initially collapsed: %s)",
  async (control, label, grouping, initiallyCollapsed) => {
    setAgentRailGrouping(grouping);
    const loadRepositoryInventory = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      project: { id: "sage", name: "Sage", roots: [] },
      projects: [{ id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 }],
      projectTransitioning: false,
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "Task",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      loadRepositoryInventory,
    });
    render(<NavigationWorkbench />);
    expect(screen.getByRole("region", { name: label })).toBeVisible();
    if (initiallyCollapsed)
      fireEvent.click(
        within(screen.getByRole("region", { name: label })).getByRole(
          "button",
          { name: label },
        ),
      );
    loadRepositoryInventory.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Toggle navigation" }));
    expect(screen.queryByRole("region", { name: label })).toBeNull();
    expect(document.querySelector(".project-sidebar__scroll")).toBeNull();
    await act(async () => {
      useWorkbenchStore.setState((state) => ({
        projectGeneration: state.projectGeneration + 1,
        tabs: state.tabs.map((tab) => ({
          ...tab,
          title: "Updated while collapsed",
        })),
      }));
    });
    expect(loadRepositoryInventory).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("button", { name: control }));
    const section = await screen.findByRole("region", { name: label });
    expect(section).toBeVisible();
    await waitFor(() =>
      expect(
        within(section).getByRole("button", { name: label }),
      ).toHaveAttribute("aria-expanded", "true"),
    );
    await waitFor(() =>
      expect(
        within(section).getByRole("button", { name: label }),
      ).toHaveFocus(),
    );
    expect(section).toContainElement(document.activeElement as HTMLElement);
    expect(
      screen.getAllByRole("button", {
        name: /Open chat Updated while collapsed/,
      }).length,
    ).toBeGreaterThan(0);
    expect(loadRepositoryInventory).toHaveBeenCalledTimes(1);
  },
);
