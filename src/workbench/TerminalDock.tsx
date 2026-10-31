import { useEffect, useMemo, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import {
  readUserTerminal,
  resizeUserTerminal,
  TERMINAL_INPUT_BYTES,
  TERMINAL_MAX_COLUMNS,
  TERMINAL_MAX_ROWS,
  terminalErrorCode,
  terminalErrorLabel,
  writeUserTerminal,
  type UserTerminalScope,
  type UserTerminalSession,
} from "../services/userTerminalDesktop";
import { utf8ByteLength } from "../domain/text";

/** How long input refused as busy keeps being offered before it is dropped. */
const INPUT_RETRY_MS = 10_000;
/** Backoff between offers of input the native side refused as busy. */
const INPUT_RETRY_FIRST_DELAY_MS = 50;
const INPUT_RETRY_MAX_DELAY_MS = 500;
/** Pause before reading again after another reader held the output. */
const READ_RETRY_DELAY_MS = 100;
const SCROLLBACK_LINES = 1_000;

interface Dimensions {
  columns: number;
  rows: number;
}

/**
 * The grid that fits `element`, from the cell size xterm rendered: its screen
 * is exactly `cols` by `rows` cells. xterm's vertical scrollbar overlays the
 * right edge, so its width stays free. Before xterm lays out, nothing changes.
 */
function dimensionsFor(element: HTMLElement, terminal: Terminal): Dimensions {
  // Fractional sizes: a rounded screen width can admit one column too many.
  const box = (selector: string) =>
    element.querySelector(selector)?.getBoundingClientRect() ?? new DOMRect();
  const screen = box(".xterm-screen");
  const fit = (space: number, cells: number, cell: number, max: number) =>
    cell > 0
      ? Math.max(1, Math.min(max, Math.floor((space * cells) / cell)))
      : cells;
  return {
    columns: fit(
      element.clientWidth - box(".scrollbar.vertical").width,
      terminal.cols,
      screen.width,
      TERMINAL_MAX_COLUMNS,
    ),
    rows: fit(
      element.clientHeight,
      terminal.rows,
      screen.height,
      TERMINAL_MAX_ROWS,
    ),
  };
}

export type TerminalDockProps = {
  scope: UserTerminalScope;
  session: UserTerminalSession;
  ended: boolean;
  onStatus: (status: string, ended?: boolean) => void;
};

export function TerminalDock({
  scope,
  session,
  ended,
  onStatus,
}: TerminalDockProps) {
  const screenRef = useRef<HTMLDivElement>(null);
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
    const element = screenRef.current;
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
      cursorBlink: true,
      scrollback: SCROLLBACK_LINES,
      // xterm measures a concrete face; Menlo keeps macOS off Courier.
      fontFamily: '"Cascadia Mono", Consolas, "SF Mono", Menlo, monospace',
      fontSize: 12,
      lineHeight: 1.3,
      theme: {
        background: "#141414",
        foreground: "#dedede",
        cursor: "#dedede",
        selectionBackground: "#444444",
      },
    });
    terminal.open(element);
    const initial = dimensionsFor(element, terminal);
    terminal.resize(initial.columns, initial.rows);

    async function pollOutput(): Promise<void> {
      let cursor = "0";
      try {
        while (!stopped) {
          let page;
          try {
            page = await readUserTerminal(ownerScope, session, cursor);
          } catch (error) {
            if (terminalErrorCode(error) === "busy") {
              await new Promise((resolve) =>
                setTimeout(resolve, READ_RETRY_DELAY_MS),
              );
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
      for (
        let delay = INPUT_RETRY_FIRST_DELAY_MS;
        ;
        delay = Math.min(delay * 2, INPUT_RETRY_MAX_DELAY_MS)
      ) {
        if (stopped) return;
        try {
          await writeUserTerminal(ownerScope, session, chunk);
          return;
        } catch (error) {
          if (
            terminalErrorCode(error) !== "busy" ||
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
            sessionEnded = terminalErrorCode(error) === "ended";
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
      if (byteLength === 0) return;
      if (pendingInputBytes + byteLength > TERMINAL_INPUT_BYTES) {
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
      const next = dimensionsFor(element, terminal);
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
      observer.disconnect();
      window.removeEventListener("resize", refreshDimensions);
      inputListener.dispose();
      terminal.dispose();
    };
  }, [ended, ownerScope, session]);

  return (
    <div className="terminal-dock__viewport">
      <div className="terminal-dock__screen" ref={screenRef} />
    </div>
  );
}
