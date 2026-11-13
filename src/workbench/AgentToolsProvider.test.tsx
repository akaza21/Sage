import {
  act,
  cleanup,
  fireEvent,
  render,
  waitFor,
  screen,
} from "@testing-library/react";
import { useContext, useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { clearAgentChatOwner, pinAgentChatOwner } from "../app/agentChatOwner";
import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { AgentToolsContext, AgentToolsProvider } from "./AgentToolsProvider";
import { useSideChatState } from "./sideChatState";
import { BrowserNewTab } from "../features/browser/BrowserNewTab";
import { useAgentChangesState } from "./agentChangesState";

vi.mock(import("../ports/nativeInvoke"), async (importOriginal) => ({
  ...(await importOriginal()),
  isTauriRuntime: () => true,
}));
const initial = useWorkbenchStore.getState();
beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "Sage", roots: [] },
    projectGeneration: 7,
    projectTransitioning: false,
    tabs: [
      {
        id: "chat-a",
        title: "Task",
        kind: "chat",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
  });
  pinAgentChatOwner("chat-a");
});
afterEach(() => {
  cleanup();
  useAgentChangesState.getState().close();
  clearAgentChatOwner();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
  useSideChatState.setState({ sides: [], pairedId: null });
});
function Launcher() {
  const { launch } = useContext(AgentToolsContext);
  return (
    <>
      <button onClick={() => launch("changes")}>Changes launcher</button>
      <button onClick={() => launch("terminal")}>Terminal launcher</button>
    </>
  );
}
it("routes Agent keybindings to the resource picker and the terminal menu to its command", async () => {
  function Host() {
    const [terminalOpen, setTerminalOpen] = useState(false);
    return (
      <AgentToolsProvider onOpenTerminal={() => setTerminalOpen(true)}>
        <Launcher />
        {terminalOpen ? <section aria-label="Opened terminal" /> : null}
      </AgentToolsProvider>
    );
  }
  render(<Host />);
  const trigger = screen.getByRole("button", { name: "Changes launcher" });
  trigger.focus();
  fireEvent.keyDown(window, { key: "G", ctrlKey: true, shiftKey: true });
  await waitFor(() =>
    expect(useAgentChangesState.getState()).toMatchObject({
      selected: true,
      generation: 7,
      owner: { projectId: "project-a", chatId: "chat-a", sessionId: null },
    }),
  );
  expect(screen.queryByRole("dialog", { name: "Changes" })).toBeNull();
  fireEvent.keyDown(window, { key: "p", ctrlKey: true });
  expect(await screen.findByRole("dialog", { name: "Files" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  fireEvent.click(screen.getByRole("button", { name: "Terminal launcher" }));
  expect(screen.getByRole("region", { name: "Opened terminal" })).toBeVisible();
});
it("offers Side chat only for a selected answer or an existing side chat", () => {
  render(
    <BrowserNewTab
      suggestions={[]}
      onNavigate={vi.fn()}
      onNewTab={vi.fn()}
      onTool={vi.fn()}
      chatAvailable
      terminalAvailable={false}
    />,
  );
  const sideChat = screen.getByRole("button", { name: "Side chat" });
  expect(sideChat).toBeDisabled();
  act(() =>
    useSideChatState.setState({
      sides: [{ id: "side-a", projectId: "project-a", parentChatId: "chat-a" }],
    }),
  );
  expect(sideChat).toBeEnabled();
});
it("dismisses an open picker when its project generation changes", async () => {
  render(
    <AgentToolsProvider>
      <Launcher />
    </AgentToolsProvider>,
  );
  fireEvent.keyDown(window, { key: "p", ctrlKey: true });
  await screen.findByRole("dialog", { name: "Files" });
  act(() => useWorkbenchStore.setState({ projectGeneration: 8 }));
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("does not resurrect a dismissed picker when returning to its Chat", async () => {
  render(
    <AgentToolsProvider>
      <Launcher />
    </AgentToolsProvider>,
  );
  fireEvent.keyDown(window, { key: "p", ctrlKey: true });
  await screen.findByRole("dialog", { name: "Files" });
  act(() => pinAgentChatOwner("other-chat"));
  act(() => pinAgentChatOwner("chat-a"));
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("reopens only this Chat's existing side chat", async () => {
  const activateTab = vi.fn(async () => undefined);
  useWorkbenchStore.setState((state) => ({
    activateTab,
    tabs: [
      ...state.tabs,
      {
        id: "side-a",
        title: "Side chat",
        kind: "chat",
        position: 1,
        pinned: false,
        dirty: false,
      },
    ],
  }));
  useSideChatState.setState({
    sides: [{ id: "side-a", projectId: "project-a", parentChatId: "chat-a" }],
    pairedId: null,
  });
  render(
    <AgentToolsProvider>
      <Launcher />
    </AgentToolsProvider>,
  );
  fireEvent.keyDown(window, { key: "s", ctrlKey: true, altKey: true });
  await vi.waitFor(() => expect(activateTab).toHaveBeenCalledWith("side-a"));
  expect(useSideChatState.getState().pairedId).toBe("side-a");
});
