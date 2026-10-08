import { useEffect, useMemo, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import {
  readUserTerminal,
  resizeUserTerminal,
  writeUserTerminal,
  type UserTerminalScope,
  type UserTerminalSession,
} from "../services/userTerminalDesktop";
import { utf8ByteLength } from "../domain/text";

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
    if (code === "invalid_scope" || code === "unknown_session")
      return "This terminal is no longer attached to the current chat.";
    if (code === "ended") return "Session ended";
  }
  return "Terminal operation failed. The native session may need cleanup.";
}

export type TerminalDockProps = {
  scope: UserTerminalScope;
  session: UserTerminalSession;
  ended: boolean;
  onStatus: (status: string) => void;
};

export function TerminalDock({
  scope,
  session,
  ended,
  onStatus,
}: TerminalDockProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const onStatusRef = useRef(onStatus);
  useEffect(() => {
    onStatusRef.current = onStatus;
  }, [onStatus]);
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
    let stopped = false;
    let sessionEnded = ended;
    let pendingInput = "";
    let pendingInputBytes = 0;
    let inputSending = false;
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

    async function pollOutput(): Promise<void> {
      let cursor = "0";
      try {
        while (!stopped) {
          let page;
          try {
            page = await readUserTerminal(ownerScope, session, cursor);
          } catch (error) {
            if (
              error &&
              typeof error === "object" &&
              "code" in error &&
              (error as { code?: unknown }).code === "busy"
            ) {
              await new Promise((resolve) => setTimeout(resolve, 100));
              continue;
            }
            throw error;
          }
          if (stopped) return;
          if (page.truncated)
            terminal.write("\r\n[Earlier terminal output was truncated]\r\n");
          if (page.bytes.length > 0) {
            await new Promise<void>((resolve) =>
              terminal.write(page.bytes, resolve),
            );
          }
          cursor = page.nextCursor;
        }
      } catch (error) {
        if (!stopped) onStatusRef.current(errorLabel(error));
      }
    }

    async function drainResizes(): Promise<void> {
      if (resizeActive) return;
      resizeActive = true;
      try {
        while (latestResize && !stopped) {
          const next = latestResize;
          latestResize = null;
          await resizeUserTerminal(
            ownerScope,
            session,
            next.columns,
            next.rows,
          );
        }
      } catch (error) {
        if (!stopped) onStatusRef.current(errorLabel(error));
      } finally {
        resizeActive = false;
      }
    }

    async function drainInput(): Promise<void> {
      if (inputSending) return;
      inputSending = true;
      try {
        while (pendingInput && !stopped && !sessionEnded) {
          const chunk = pendingInput;
          pendingInput = "";
          pendingInputBytes = 0;
          try {
            await writeUserTerminal(ownerScope, session, chunk);
          } catch (error) {
            pendingInput = "";
            pendingInputBytes = 0;
            if (error && typeof error === "object" && "code" in error)
              sessionEnded = (error as { code?: unknown }).code === "ended";
            if (!stopped) onStatusRef.current(errorLabel(error));
            break;
          }
        }
      } finally {
        inputSending = false;
      }
    }

    const inputListener = terminal.onData((data) => {
      if (stopped || sessionEnded) return;
      const byteLength = utf8ByteLength(data);
      if (byteLength === 0) return;
      if (byteLength > 4_096 || pendingInputBytes + byteLength > 4_096) {
        onStatusRef.current(
          "Input was not sent: terminal input buffer limit reached.",
        );
        return;
      }
      pendingInput += data;
      pendingInputBytes += byteLength;
      void drainInput();
    });
    const refreshDimensions = () => {
      if (stopped) return;
      if (window.devicePixelRatio !== observedPixelRatio) {
        observedPixelRatio = window.devicePixelRatio;
        terminal.refresh(0, terminal.rows - 1);
      }
      const next = dimensionsFor(element);
      if (terminal.cols === next.columns && terminal.rows === next.rows) return;
      terminal.resize(next.columns, next.rows);
      latestResize = next;
      void drainResizes();
    };
    const observer = new ResizeObserver(refreshDimensions);
    observer.observe(element);
    window.addEventListener("resize", refreshDimensions);
    void resizeUserTerminal(
      ownerScope,
      session,
      initial.columns,
      initial.rows,
    ).catch((error: unknown) => {
      if (!stopped) onStatusRef.current(errorLabel(error));
    });
    void pollOutput();
    terminal.focus();
    return () => {
      stopped = true;
      observer.disconnect();
      window.removeEventListener("resize", refreshDimensions);
      inputListener.dispose();
      terminal.dispose();
    };
  }, [ended, ownerScope, session]);

  return <div className="terminal-dock__viewport" ref={viewportRef} />;
}
