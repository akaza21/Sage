import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { LeftSidebar } from "./LeftSidebar";
import {
  resetAgentRailViewPrefs,
  setAgentRailOrganize,
} from "./agentRailViewPrefs";
import { resetAgentRailInteractions } from "./agentRailInteractions";
import { chatDisplayTitle } from "../domain/chatTitle";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  resetAgentRailViewPrefs();
  resetAgentRailInteractions();
  useWorkbenchStore.setState(initial, true);
});

it.each(["project", "list"] as const)(
  "restores an archived active project-less chat in %s without switching projects",
  async (organize) => {
    resetBrowserDesktopForTests();
    useWorkbenchStore.setState(initial, true);
    await useWorkbenchStore.getState().initialize();
    await useWorkbenchStore.getState().createChat(null);
    useWorkbenchStore.setState({
      tabs: useWorkbenchStore
        .getState()
        .tabs.map((tab) => ({ ...tab, updatedAt: Date.now() })),
    });
    const { activeTabId: chat, project } = useWorkbenchStore.getState();
    const title = chatDisplayTitle(
      useWorkbenchStore.getState().tabs.find((tab) => tab.id === chat)!.title,
    );
    setAgentRailOrganize(organize);
    render(<LeftSidebar />);
    fireEvent.click(
      await screen.findByRole("button", { name: `Archive ${title}` }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Archived chats: 1" }),
    );
    expect(
      screen.getByRole("button", { name: `Delete chat ${title}` }),
    ).toBeVisible();
    expect(useWorkbenchStore.getState().project?.id).toBe(project!.id);
    fireEvent.click(
      screen.getByRole("button", { name: `Restore chat ${title}` }),
    );
    await waitFor(() =>
      expect(
        useWorkbenchStore.getState().tabs.some((tab) => tab.id === chat),
      ).toBe(true),
    );
    await act(async () => undefined);
    expect(
      screen.queryByRole("button", { name: `Restore chat ${title}` }),
    ).toBeNull();
    expect(useWorkbenchStore.getState().project?.id).toBe(project!.id);
  },
);
