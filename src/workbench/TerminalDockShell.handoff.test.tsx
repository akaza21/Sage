import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { useTerminalDockStore } from "../app/terminalDockStore";
import {
  ComposerTerminalCatch,
  catchTerminalCommand,
} from "../features/chat/ComposerTerminalCatch";
import TerminalDockForWorkbench from "./TerminalDockShell";
import { connectTerminalDock } from "./terminalDockCommands";
import { DISCONNECTED_TERMINAL } from "./terminalDockProjection";
import { AgentToolsProvider } from "./AgentToolsProvider";

const backend = vi.hoisted(() => ({
  sessions: new Map<string, { scope: unknown; input: string }>(),
  sequence: 0,
  failWrite: false,
}));
vi.mock("../ports/nativeInvoke", () => ({
  isTauriRuntime: () => true,
  invokeNative: async (
    command: string,
    args?: {
      request?: { scope: unknown; handle: { sessionId: string }; text: string };
    },
  ) => {
    const request = args?.request;
    if (command === "get_terminal_dock_visible") return false;
    if (command === "set_terminal_dock_visible") return;
    if (command === "terminal_create") {
      const sessionId = `f99cd56a-32d9-4600-a1f6-${String(++backend.sequence).padStart(12, "0")}`;
      backend.sessions.set(sessionId, { scope: request!.scope, input: "" });
      return { sessionId, revision: "1", shellPath: "shell" };
    }
    if (command === "terminal_write") {
      if (backend.failWrite) throw { code: "busy" };
      backend.sessions.get(request!.handle.sessionId)!.input += request!.text;
      return;
    }
    if (command === "terminal_close") {
      backend.sessions.delete(request!.handle.sessionId);
      return;
    }
    throw new Error(`Unexpected terminal command: ${command}`);
  },
}));
// Rendering is exercised in the live Playwright check; keep the actual native
// creation and input adapters here, with only their backend replaced.
vi.mock("./TerminalDock", () => ({
  default: () => <div>Terminal screen</div>,
}));

const original = useWorkbenchStore.getState();
const text = 'agent --resume "native-1"';
let disconnect: (() => void) | undefined;
afterEach(() => {
  cleanup();
  disconnect?.();
  disconnect = undefined;
  useWorkbenchStore.setState(original, true);
  useTerminalDockStore.setState({ visible: false, ready: false });
  backend.sessions.clear();
  backend.failWrite = false;
});

async function mount() {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] },
    projectTransitioning: false,
    nativeTransitionEpoch: 2,
    tabs: [
      {
        id: "chat-1",
        kind: "chat",
        title: "Chat",
        dirty: false,
        position: 0,
        pinned: false,
      },
    ],
    activeTabId: "chat-1",
  });
  function Host() {
    const fieldRef = useRef<HTMLTextAreaElement>(null);
    const [status, setStatus] = useState("");
    const visible = useTerminalDockStore((state) => state.visible);
    return (
      <>
        <textarea ref={fieldRef} defaultValue="/hooks" />
        <AgentToolsProvider onOpenTerminal={() => terminal.open()}>
          <ComposerTerminalCatch
            chatId="chat-1"
            fieldRef={fieldRef}
            onAnnounce={setStatus}
            text="/hooks"
          />
        </AgentToolsProvider>
        <span role="status">{status}</span>
        <TerminalDockForWorkbench
          container={document.body}
          pinnedChatId="chat-1"
          visible={visible}
          open={visible}
          setOpen={(open) => useTerminalDockStore.getState().setVisible(open)}
        />
      </>
    );
  }
  let terminal = DISCONNECTED_TERMINAL;
  disconnect = connectTerminalDock(
    () => false,
    (projection) => (terminal = projection),
  );
  render(<Host />);
  await waitFor(() => expect(useTerminalDockStore.getState().ready).toBe(true));
}

const catchCommand = (command: string) =>
  act(() => {
    catchTerminalCommand("chat-1", { input: "/hooks", name: "hooks", command });
  });

it("opens a caught command as unrun input in its chat's project terminal and clears the choice", async () => {
  await mount();
  // A command that would press Enter is never handed to the terminal.
  catchCommand(text + "\n");
  fireEvent.click(screen.getByRole("button", { name: "Open in terminal" }));
  expect(screen.getByRole("alert")).toHaveTextContent("cannot be typed");
  expect(useTerminalDockStore.getState().visible).toBe(false);
  catchCommand(text);
  fireEvent.click(screen.getByRole("button", { name: "Open in terminal" }));
  await waitFor(() =>
    expect(
      screen.queryByRole("group", { name: "Terminal-only command" }),
    ).toBeNull(),
  );
  expect(screen.getByRole("region", { name: "Terminal" })).toBeVisible();
  expect([...backend.sessions.values()]).toEqual([
    {
      scope: {
        projectId: "project-1",
        projectGeneration: "2",
        chatId: "chat-1",
      },
      input: text,
    },
  ]);
  expect(screen.getByRole("textbox")).toHaveValue("/hooks");
  expect(useWorkbenchStore.getState().terminalHandoff).toBeNull();
});

// Stores a request in the workbench slot and shows the dock, as the composer
// does through the Agent tools seam; the shell takes it.
async function handOff(input: string) {
  let settled!: Promise<void>;
  act(() => {
    settled = new Promise<void>((resolve, reject) => {
      useWorkbenchStore.setState({
        terminalHandoff: {
          text: input,
          finish: (error) => (error ? reject(new Error(error)) : resolve()),
        },
      });
      useTerminalDockStore.getState().setVisible(true);
    });
  });
  // Await outside act so React can commit the request the dock takes.
  return settled;
}

it("writes each handoff to a new tab, keeps a tab whose write failed, and holds a handoff at capacity", async () => {
  await mount();
  await handOff(text);
  await handOff(text);
  backend.failWrite = true;
  await expect(handOff(text)).rejects.toThrow("not reading");
  backend.failWrite = false;
  await expect(handOff("x".repeat(4097))).rejects.toThrow("failed");
  await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(4));
  expect(
    [...backend.sessions.values()].map((session) => session.input),
  ).toEqual([text, text, "", ""]);
  // At the tab limit the request stays pending for its sender to time out.
  void handOff(text);
  await act(() => new Promise((resolve) => setTimeout(resolve, 20)));
  expect(useWorkbenchStore.getState().terminalHandoff?.text).toBe(text);
  expect(backend.sessions.size).toBe(4);
});
