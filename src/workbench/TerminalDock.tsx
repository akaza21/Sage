import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import {
  closeUserTerminal,
  createUserTerminal,
  readUserTerminal,
  resizeUserTerminal,
  writeUserTerminal,
  type UserTerminalScope,
  type UserTerminalSession,
} from "../services/userTerminalDesktop";
import "./terminal-dock.css";
import { useWorkbenchStore } from "../app/store";
import { utf8ByteLength } from "../domain/text";
import { firstChatTabId } from "./workbenchWindowMode";

interface Dimensions {
  columns: number;
  rows: number;
}

function dimensionsFor(element: HTMLElement): Dimensions {
  return {
    columns: Math.max(1, Math.min(500, Math.floor(element.clientWidth / 7.3))),
    rows: Math.max(1, Math.min(300, Math.floor(element.clientHeight / 16))),
  };
}

function errorLabel(error: unknown): string {
  if (error && typeof error === "object" && "code" in error) {
    const code = (error as { code?: unknown }).code;
    if (code === "unavailable")
      return "Terminal is unavailable on this platform.";
    if (code === "capacity")
      return "Terminal limit reached for this chat or project.";
    if (code === "invalid_scope" || code === "unknown_session") {
      return "This terminal is no longer attached to the current chat.";
    }
    if (code === "ended") return "Session ended";
  }
  return "Terminal operation failed. The native session may need cleanup.";
}

function selectedScope(
  pinnedChatId: string | null,
  preferredChatId?: string,
): UserTerminalScope | null {
  const state = useWorkbenchStore.getState();
  if (!state.project || state.projectTransitioning) return null;
  const activeChat = state.tabs.find(
    (tab) => tab.id === state.activeTabId && tab.kind === "chat",
  )?.id;
  const chatId =
    activeChat ??
    (state.tabs.some((tab) => tab.kind === "chat" && tab.id === preferredChatId)
      ? preferredChatId
      : state.tabs.some((tab) => tab.kind === "chat" && tab.id === pinnedChatId)
        ? pinnedChatId
        : firstChatTabId(state.tabs));
  if (!chatId) return null;
  return {
    projectId: state.project.id,
    projectGeneration: String(state.nativeTransitionEpoch),
    chatId,
  };
}

export function TerminalDockForWorkbench({
  setOpen,
  pinnedChatId,
}: {
  setOpen: (open: boolean) => void;
  pinnedChatId: string | null;
}) {
  const onClose = useCallback(() => setOpen(false), [setOpen]);
  const [uiProjectGeneration] = useState(
    () => useWorkbenchStore.getState().projectGeneration,
  );
  const [scope] = useState(() => selectedScope(pinnedChatId));
  useEffect(() => {
    if (!scope) return;
    return useWorkbenchStore.subscribe(() => {
      const current = selectedScope(pinnedChatId, scope.chatId);
      if (
        !current ||
        useWorkbenchStore.getState().projectGeneration !==
          uiProjectGeneration ||
        current.projectId !== scope.projectId ||
        current.projectGeneration !== scope.projectGeneration ||
        current.chatId !== scope.chatId
      ) {
        onClose();
      }
    });
  }, [scope, onClose, pinnedChatId, uiProjectGeneration]);
  if (!scope) {
    return (
      <section className="terminal-dock" aria-label="Terminal">
        <div className="terminal-dock__header">Terminal unavailable</div>
        <p className="terminal-dock__message" role="status">
          Open a chat in an active project to use the terminal.
        </p>
        <button type="button" onClick={onClose}>
          Close terminal
        </button>
      </section>
    );
  }
  return <TerminalDock scope={scope} onClose={onClose} />;
}

export function TerminalDock({
  scope,
  onClose,
}: {
  scope: UserTerminalScope;
  onClose: () => void;
}) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<(() => Promise<void>) | null>(null);
  const [shellPath, setShellPath] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [closing, setClosing] = useState(false);
  const ownerScope = useMemo(
    () => ({
      projectId: scope.projectId,
      projectGeneration: scope.projectGeneration,
      chatId: scope.chatId,
    }),
    [scope.projectId, scope.projectGeneration, scope.chatId],
  );

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const viewport: HTMLDivElement = element;
    let stopped = false;
    let session: UserTerminalSession | null = null;
    let observer: ResizeObserver | null = null;
    let windowResize: (() => void) | null = null;
    let inputListener: { dispose(): void } | null = null;
    let pendingInput = "";
    let pendingInputBytes = 0;
    let inputSending = false;
    let ended = false;
    let resizeActive = false;
    let latestResize: Dimensions | null = null;
    let observedPixelRatio = window.devicePixelRatio;
    const terminal = new Terminal({
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: true,
      fontFamily: "Cascadia Mono, Consolas, monospace",
      fontSize: 12,
      lineHeight: 1.3,
      scrollback: 1_000,
      theme: {
        background: "#141414",
        foreground: "#dedede",
        cursor: "#dedede",
        selectionBackground: "#444444",
      },
    });
    terminal.open(element);
    const initial = dimensionsFor(element);
    terminal.resize(initial.columns, initial.rows);

    async function writeOutput(bytes: Uint8Array): Promise<void> {
      await new Promise<void>((resolve) => terminal.write(bytes, resolve));
    }

    async function pollOutput(owner: UserTerminalSession): Promise<void> {
      let cursor = "0";
      while (!stopped && session === owner) {
        const page = await readUserTerminal(ownerScope, owner, cursor);
        if (stopped || session !== owner) return;
        if (page.truncated) {
          terminal.write("\r\n[Earlier terminal output was truncated]\r\n");
        }
        if (page.bytes.length > 0) await writeOutput(page.bytes);
        cursor = page.nextCursor;
      }
    }

    async function drainResizes(owner: UserTerminalSession): Promise<void> {
      if (resizeActive) return;
      resizeActive = true;
      try {
        while (latestResize && !stopped && session === owner) {
          const next = latestResize;
          latestResize = null;
          await resizeUserTerminal(ownerScope, owner, next.columns, next.rows);
        }
      } catch (error) {
        if (!stopped) setMessage(errorLabel(error));
      } finally {
        resizeActive = false;
      }
    }

    async function drainInput(owner: UserTerminalSession): Promise<void> {
      if (inputSending) return;
      inputSending = true;
      try {
        while (pendingInput && !stopped && session === owner && !ended) {
          const chunk = pendingInput;
          pendingInput = "";
          pendingInputBytes = 0;
          try {
            await writeUserTerminal(ownerScope, owner, chunk);
          } catch (error) {
            pendingInput = "";
            pendingInputBytes = 0;
            if (error && typeof error === "object" && "code" in error) {
              ended = (error as { code?: unknown }).code === "ended";
            }
            if (!stopped) setMessage(errorLabel(error));
            break;
          }
        }
      } finally {
        inputSending = false;
      }
    }

    async function start(): Promise<void> {
      try {
        const owner = await createUserTerminal(
          ownerScope,
          initial.columns,
          initial.rows,
        );
        if (stopped) {
          await closeUserTerminal(ownerScope, owner).catch(() => undefined);
          return;
        }
        session = owner;
        setShellPath(owner.shellPath);
        terminal.focus();
        inputListener = terminal.onData((data) => {
          if (stopped || session !== owner || ended) return;
          const byteLength = utf8ByteLength(data);
          if (byteLength === 0) return;
          if (byteLength > 4_096 || pendingInputBytes + byteLength > 4_096) {
            setMessage(
              "Input was not sent: terminal input buffer limit reached.",
            );
            return;
          }
          pendingInput += data;
          pendingInputBytes += byteLength;
          void drainInput(owner);
        });
        const refreshDimensions = () => {
          if (stopped || session !== owner) return;
          if (window.devicePixelRatio !== observedPixelRatio) {
            observedPixelRatio = window.devicePixelRatio;
            terminal.refresh(0, terminal.rows - 1);
          }
          const next = dimensionsFor(viewport);
          if (terminal.cols === next.columns && terminal.rows === next.rows)
            return;
          terminal.resize(next.columns, next.rows);
          latestResize = next;
          void drainResizes(owner);
        };
        observer = new ResizeObserver(refreshDimensions);
        observer.observe(viewport);
        window.addEventListener("resize", refreshDimensions);
        windowResize = refreshDimensions;
        await pollOutput(owner);
      } catch (error) {
        if (!stopped) setMessage(errorLabel(error));
      }
    }

    closeRef.current = async () => {
      if (!session) {
        onClose();
        return;
      }
      setClosing(true);
      try {
        await closeUserTerminal(ownerScope, session);
        session = null;
        onClose();
      } catch (error) {
        setClosing(false);
        setMessage(errorLabel(error));
      }
    };
    void start();
    return () => {
      stopped = true;
      closeRef.current = null;
      observer?.disconnect();
      if (windowResize) window.removeEventListener("resize", windowResize);
      inputListener?.dispose();
      terminal.dispose();
      if (session)
        void closeUserTerminal(ownerScope, session).catch(() => undefined);
    };
  }, [ownerScope, onClose]);

  return (
    <section className="terminal-dock" aria-label="Terminal">
      <div className="terminal-dock__header">
        <div className="terminal-dock__tab" title={shellPath ?? undefined}>
          <span aria-hidden="true">▣</span>
          <span>{shellPath ?? "Starting terminal…"}</span>
        </div>
        <button
          className="terminal-dock__close"
          type="button"
          aria-label="Close terminal"
          disabled={closing}
          onClick={() => void closeRef.current?.()}
        >
          {closing ? "Closing…" : "×"}
        </button>
      </div>
      {message ? (
        <p className="terminal-dock__message" role="status">
          {message}
        </p>
      ) : null}
      <div className="terminal-dock__viewport" ref={viewportRef} />
    </section>
  );
}
