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
import { useAgentPanelView } from "./agentPanelView";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { clearChatSearchFolderRequest } from "../features/chatSearch/chatSearchScope";
import AgentWorkbenchBody from "./AgentWorkbenchBodyView";
import { agentWorkbenchLayout } from "./agentWorkbenchLayout";
import { resetAgentRailViewPrefs } from "./agentRailViewPrefs";

vi.mock("./AgentChatPanes", () => ({ AgentChatPanes: () => null }));
vi.mock("./AgentArtifactPane", () => ({ AgentArtifactPane: () => null }));

const initial = useWorkbenchStore.getState();
afterEach(() => {
  useAgentPanelView.setState({ view: "home" });
  cleanup();
  useAgentSettingsState.getState().close();
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
        centerVisible
        artifactMaximized={false}
        onArtifactMaximizedChange={() => undefined}
        artifactExpanded={false}
        onArtifactExpandedChange={() => undefined}
        onAgentLeftRailWidthChange={() => undefined}
        onAgentArtifactWidthChange={() => undefined}
        onOpenArtifactInEditor={() => undefined}
      />
    </>
  );
}

it("opens Agent settings from collapsed navigation without opening an Editor tab", async () => {
  const openSettings = vi.fn();
  useWorkbenchStore.setState({ openSettings });
  render(<NavigationWorkbench />);
  fireEvent.click(screen.getByRole("button", { name: "Toggle navigation" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));
  expect(useAgentSettingsState.getState().page).toBe("Appearance");
  expect(openSettings).not.toHaveBeenCalled();
});

it.each([
  ["Projects", "Projects", false],
  ["Projects", "Projects", true],
] as const)(
  "the %s rail item brings back the %s list (section initially collapsed: %s)",
  async (control, label, initiallyCollapsed) => {
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
    fireEvent.click(
      within(
        await screen.findByRole("navigation", { name: "Sidebar sections" }),
      ).getByRole("button", { name: control }),
    );
    const section = await screen.findByRole("region", { name: label });
    expect(section).toBeVisible();
    await waitFor(() =>
      expect(
        within(section).getByRole("button", { name: label }),
      ).toHaveAttribute("aria-expanded", "true"),
    );
    expect(
      (
        await screen.findAllByRole("button", {
          name: /Open chat Updated while collapsed/,
        })
      ).length,
    ).toBeGreaterThan(0);
    expect(loadRepositoryInventory).toHaveBeenCalledTimes(1);
  },
);
