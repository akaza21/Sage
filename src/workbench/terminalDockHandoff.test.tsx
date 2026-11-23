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
import { ComposerTerminalCatch } from "../features/chat/ComposerTerminalCatch";
import { catchTerminalCommand } from "../services/terminalHandoff";
import TerminalDockForWorkbench from "./TerminalDockShell";
import { connectTerminalDock } from "./terminalDockCommands";
import { openTerminalWith } from "./terminalDockHandoff";

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
  useTerminalDockStore.setState({
    visible: false,
    ready: false,
    pendingOpen: null,
  });
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
        <ComposerTerminalCatch
          chatId="chat-1"
          fieldRef={fieldRef}
          onAnnounce={setStatus}
          text="/hooks"
        />
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
  disconnect = connectTerminalDock(
    () => false,
    () => undefined,
  );
  render(<Host />);
  await waitFor(() => expect(useTerminalDockStore.getState().ready).toBe(true));
}

it("opens a caught command in its project terminal through the dock API and clears the choice", async () => {
  await mount();
  act(() => {
    catchTerminalCommand("chat-1", {
      input: "/hooks",
      name: "hooks",
      command: text,
    });
  });
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
  expect(useTerminalDockStore.getState().pendingOpen).toBeNull();
});

it("creates a new tab with exact input and rejects unsafe input, capacity and write failures", async () => {
  await mount();
  // Await outside act so React can commit the pending request it consumes.
  const start = () => openTerminalWith({ chatId: "chat-1", text });
  let result!: Promise<void>;
  act(() => {
    result = start();
  });
  await result;
  act(() => {
    result = start();
  });
  await result;
  await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
  expect(
    [...backend.sessions.values()].map((session) => session.input),
  ).toEqual([text, text]);
  for (const value of [
    text + "\n",
    text + "\r",
    "\u001b[200~",
    "x".repeat(4097),
  ]) {
    await expect(
      openTerminalWith({ chatId: "chat-1", text: value }),
    ).rejects.toThrow("one line");
  }
  backend.failWrite = true;
  act(() => {
    result = start();
  });
  await expect(result).rejects.toThrow("not reading");
  expect(backend.sessions.size).toBe(2);
  backend.failWrite = false;
  for (let index = 0; index < 2; index++) {
    act(() => {
      result = start();
    });
    await result;
  }
  act(() => {
    result = start();
  });
  await expect(result).rejects.toThrow("limit");
  expect(backend.sessions.size).toBe(4);
  expect(useTerminalDockStore.getState().pendingOpen).toBeNull();
});
