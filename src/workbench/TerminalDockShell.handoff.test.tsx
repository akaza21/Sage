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
let closeOther: () => void = () => undefined;
let mountDock: () => void = () => undefined;
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

async function mount(docked = true) {
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
      {
        id: "chat-2",
        kind: "chat",
        title: "Other chat",
        dirty: false,
        position: 1,
        pinned: false,
      },
    ],
    activeTabId: "chat-1",
  });
  function Host() {
    const fieldRef = useRef<HTMLTextAreaElement>(null);
    const otherRef = useRef<HTMLTextAreaElement>(null);
    const [status, setStatus] = useState("");
    const [other, setOther] = useState(true);
    closeOther = () => setOther(false);
    // The real dock is lazy: it may mount only after a request opened it.
    const [dock, setDock] = useState(docked);
    mountDock = () => setDock(true);
    const visible = useTerminalDockStore((state) => state.visible);
    // As in a single Agent pane, chat-1's field shows only while it is active.
    const away = useWorkbenchStore((state) => state.activeTabId === "chat-2");
    return (
      <>
        <textarea ref={fieldRef} aria-label="Chat" defaultValue="/hooks" />
        <textarea ref={otherRef} aria-label="Other" defaultValue="/hooks" />
        <AgentToolsProvider onOpenTerminal={() => terminal.open()}>
          {away ? null : (
            <ComposerTerminalCatch
              chatId="chat-1"
              fieldRef={fieldRef}
              onAnnounce={setStatus}
              text="/hooks"
            />
          )}
          {other ? (
            <ComposerTerminalCatch
              chatId="chat-2"
              fieldRef={otherRef}
              onAnnounce={() => undefined}
              text="/hooks"
            />
          ) : null}
        </AgentToolsProvider>
        <span role="status">{status}</span>
        {dock ? (
          <TerminalDockForWorkbench
            container={document.body}
            pinnedChatId="chat-1"
            visible={visible}
            open={visible}
            setOpen={(open) => useTerminalDockStore.getState().setVisible(open)}
          />
        ) : null}
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

const catchCommand = (command: string, chatId = "chat-1") =>
  act(() => {
    catchTerminalCommand(chatId, { input: "/hooks", name: "hooks", command });
  });

it("never opens a dock that had not mounted for the chat the user went to, and a released choice stays gone", async () => {
  await mount(false);
  catchCommand(text);
  fireEvent.click(screen.getByRole("button", { name: "Open in terminal" }));
  expect(useTerminalDockStore.getState().visible).toBe(true);
  // Side by side, chat-1's field stays mounted while chat-3 becomes active.
  act(() =>
    useWorkbenchStore.setState(({ tabs }) => ({
      tabs: [...tabs, { ...tabs[1]!, id: "chat-3", position: 2 }],
      activeTabId: "chat-3",
    })),
  );
  expect(screen.getByRole("alert")).toHaveTextContent("left this chat");
  expect(useTerminalDockStore.getState().visible).toBe(false);
  act(() => mountDock());
  await act(() => new Promise((resolve) => setTimeout(resolve, 20)));
  expect(screen.queryByRole("region", { name: "Terminal" })).toBeNull();
  expect(backend.sessions.size).toBe(0);
  // Sending as text releases the cancelled choice: the field mounting again
  // shows nothing.
  fireEvent.click(screen.getByRole("button", { name: "Send as text anyway" }));
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-2" }));
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-1" }));
  expect(
    screen.queryByRole("group", { name: "Terminal-only command" }),
  ).toBeNull();
});

it("opens a caught command as unrun input in its chat's project terminal and clears the choice", async () => {
  await mount();
  // A command that would press Enter is never handed to the terminal.
  catchCommand(text + "\n");
  fireEvent.click(screen.getByRole("button", { name: "Open in terminal" }));
  expect(screen.getByRole("alert")).toHaveTextContent("did not take");
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
  expect(screen.getByRole("textbox", { name: "Chat" })).toHaveValue("/hooks");
  expect(useWorkbenchStore.getState().terminalHandoff).toBeNull();
  // The dock shows chat-1, so another chat's request is left untyped, and its
  // field takes it with it when it unmounts.
  catchCommand("other --resume", "chat-2");
  fireEvent.click(screen.getByRole("button", { name: "Open in terminal" }));
  await act(() => new Promise((resolve) => setTimeout(resolve, 20)));
  expect(useWorkbenchStore.getState().terminalHandoff?.chatId).toBe("chat-2");
  expect(backend.sessions.size).toBe(1);
  act(() => closeOther());
  expect(useWorkbenchStore.getState().terminalHandoff).toBeNull();
  // Switching chats before the dock takes a request ends it at once; on the
  // way back the field says why, and the dock neither reopens nor types.
  catchCommand(text);
  fireEvent.click(screen.getByRole("button", { name: "Open in terminal" }));
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-2" }));
  expect(useWorkbenchStore.getState().terminalHandoff).toBeNull();
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-1" }));
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Open in terminal was cancelled when you left this chat.",
  );
  await act(() => new Promise((resolve) => setTimeout(resolve, 20)));
  expect(useTerminalDockStore.getState().visible).toBe(false);
  // The closed dock took its tabs with it, and none was opened again.
  expect(backend.sessions.size).toBe(0);
  // Side by side, the field stays mounted while another chat becomes active.
  // A later open that succeeds supersedes the cancelled choice, so it never
  // comes back when the field mounts again.
  catchCommand(text);
  fireEvent.click(screen.getByRole("button", { name: "Open in terminal" }));
  act(() =>
    useWorkbenchStore.setState(({ tabs }) => ({
      tabs: [...tabs, { ...tabs[1]!, id: "chat-3", position: 2 }],
      activeTabId: "chat-3",
    })),
  );
  expect(screen.getByRole("alert")).toHaveTextContent("left this chat");
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-1" }));
  fireEvent.click(screen.getByRole("button", { name: "Open in terminal" }));
  await waitFor(() =>
    expect(
      screen.queryByRole("group", { name: "Terminal-only command" }),
    ).toBeNull(),
  );
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-2" }));
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-1" }));
  expect(
    screen.queryByRole("group", { name: "Terminal-only command" }),
  ).toBeNull();
});

// Stores a request in the workbench slot and shows the dock, as the composer
// does through the Agent tools seam; the shell takes it.
async function handOff(input: string) {
  let settled!: Promise<void>;
  act(() => {
    settled = new Promise<void>((resolve, reject) => {
      useWorkbenchStore.setState({
        terminalHandoff: {
          projectId: "project-1",
          projectGeneration: "2",
          chatId: "chat-1",
          run: (type) => {
            useWorkbenchStore.setState({ terminalHandoff: null });
            if (type) void type(input).then(resolve, reject);
            else reject(new Error("no tab"));
          },
        },
      });
      useTerminalDockStore.getState().setVisible(true);
    });
  });
  // Await outside act so React can commit the request the dock takes.
  return settled;
}

it("writes each handoff to a new tab, keeps a tab whose write failed, and refuses one at capacity at once", async () => {
  await mount();
  await handOff(text);
  await handOff(text);
  backend.failWrite = true;
  await expect(handOff(text)).rejects.toMatchObject({ code: "busy" });
  backend.failWrite = false;
  await expect(handOff("x".repeat(4097))).rejects.toThrow("4096");
  await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(4));
  expect(
    [...backend.sessions.values()].map((session) => session.input),
  ).toEqual([text, text, "", ""]);
  // At the tab limit the dock answers at once, without opening a session.
  await expect(handOff(text)).rejects.toThrow("no tab");
  expect(backend.sessions.size).toBe(4);
});
