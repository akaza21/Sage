import { useEffect, useRef } from "react";
import { Terminal, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import {
  readUserTerminal,
  resizeUserTerminal,
  terminalErrorLabel,
  writeUserTerminal,
  type UserTerminalScope,
  type UserTerminalSession,
} from "../services/userTerminalDesktop";
import { utf8ByteLength } from "../domain/text";

/** How long input refused as busy keeps being offered before it is dropped. */
const INPUT_RETRY_MS = 10_000;

function errorCode(error: unknown): unknown {
  return error !== null && typeof error === "object"
    ? (error as { code?: unknown }).code
    : undefined;
}

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

export type TerminalDockProps = {
  scope: UserTerminalScope;
  session: UserTerminalSession;
  ended: boolean;
  onStatus: (status: string, ended?: boolean) => void;
  getTheme: (element: HTMLElement) => ITheme;
};

export function TerminalDock({
  scope,
  session,
  ended,
  onStatus,
  getTheme,
}: TerminalDockProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const onStatusRef = useRef(onStatus);
  useEffect(() => {
    onStatusRef.current = onStatus;
  }, [onStatus]);
  const { projectId, projectGeneration, chatId } = scope;

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const ownerScope = { projectId, projectGeneration, chatId };
    let stopped = false;
    let sessionEnded = ended;
    let pendingInput = "";
    let pendingInputBytes = 0;
    let inputSending = false;
    let resizeActive = false;
    let latestResize: Dimensions | null = null;
    let observedPixelRatio = window.devicePixelRatio;
    const terminal = new Terminal({
      cursorBlink: true,
      // xterm measures a concrete face; Menlo keeps macOS off Courier.
      fontFamily: '"Cascadia Mono", Consolas, "SF Mono", Menlo, monospace',
      fontSize: 12,
      lineHeight: 1.3,
      scrollback: 1_000,
      theme: getTheme(element),
    });
    terminal.open(element);
    const themeObserver = new MutationObserver(() => {
      terminal.options.theme = getTheme(element);
    });
    for (const host of document.querySelectorAll("html, body, .workbench"))
      themeObserver.observe(host, {
        attributes: true,
        attributeFilter: ["style", "class"],
      });
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
            if (errorCode(error) === "busy") {
              await new Promise((resolve) => setTimeout(resolve, 100));
              continue;
            }
            throw error;
          }
          if (stopped) return;
          if (page.truncated)
            terminal.write("\r\n[Earlier terminal output was truncated]\r\n");
          if (page.bytes.length) {
            await new Promise<void>((resolve) =>
              terminal.write(page.bytes, resolve),
            );
          }
          cursor = page.nextCursor;
        }
      } catch (error) {
        if (!stopped) onStatusRef.current(terminalErrorLabel(error));
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
        if (!stopped) onStatusRef.current(terminalErrorLabel(error));
      } finally {
        resizeActive = false;
      }
    }

    // A program that is not reading its input makes the native side refuse
    // a write as busy without taking any of it, so the same chunk is sent
    // again after a growing pause, for up to INPUT_RETRY_MS.
    async function writeWhenAccepted(chunk: string): Promise<void> {
      const started = Date.now();
      for (let delay = 50; ; delay = Math.min(delay * 2, 500)) {
        try {
          await writeUserTerminal(ownerScope, session, chunk);
          return;
        } catch (error) {
          if (
            errorCode(error) !== "busy" ||
            stopped ||
            Date.now() - started >= INPUT_RETRY_MS
          )
            throw error;
        }
        await new Promise((resolve) => setTimeout(resolve, delay));
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
            await writeWhenAccepted(chunk);
          } catch (error) {
            pendingInput = "";
            pendingInputBytes = 0;
            sessionEnded = errorCode(error) === "ended";
            if (!stopped)
              onStatusRef.current(terminalErrorLabel(error), sessionEnded);
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
      if (!byteLength) return;
      if (pendingInputBytes + byteLength > 4_096) {
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
      if (!stopped) onStatusRef.current(terminalErrorLabel(error));
    });
    void pollOutput();
    terminal.focus();
    return () => {
      stopped = true;
      themeObserver.disconnect();
      observer.disconnect();
      window.removeEventListener("resize", refreshDimensions);
      inputListener.dispose();
      terminal.dispose();
    };
  }, [ended, projectId, projectGeneration, chatId, session, getTheme]);

  return <div className="terminal-dock__viewport" ref={viewportRef} />;
}

export default TerminalDock;
