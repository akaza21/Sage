import { useEffect, useEffectEvent, useRef } from "react";
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
import { withStyleNonce } from "../ports/styleNonce";

/** How long input refused as busy keeps being offered before it is dropped. */
const INPUT_RETRY_MS = 10_000;
/** Backoff between offers of input the native side refused as busy. */
const INPUT_RETRY_FIRST_DELAY_MS = 50;
const INPUT_RETRY_MAX_DELAY_MS = 500;
/** Pause before reading again after another reader held the output. */
const READ_RETRY_DELAY_MS = 100;
const SCROLLBACK_LINES = 1_000;
/** The only system setting that redefines the theme tokens. */
const FORCED_COLORS_QUERY = "(forced-colors: active)";
// xterm requires two columns for wide characters and at least one row.
const MIN_COLUMNS = 2;
const MIN_ROWS = 1;

type Dimensions = readonly [columns: number, rows: number];

/** xterm 6 has no public cell metrics API. Keep this private seam local. */
type MeasuredTerminal = Terminal & {
  _core: {
    _renderService: {
      dimensions: { device: { cell: { width: number; height: number } } };
    };
  };
};

export type TerminalDockProps = {
  scope: UserTerminalScope;
  session: UserTerminalSession;
  ended: boolean;
  visible?: boolean;
  onStatus: (status: string, ended?: boolean) => void;
};

export function TerminalDock({
  scope,
  session,
  ended,
  onStatus,
  visible = true,
}: TerminalDockProps) {
  const screenRef = useRef<HTMLDivElement>(null);
  const reportStatus = useEffectEvent(onStatus);
  const displayRef = useRef<((visible: boolean) => void) | null>(null);

  useEffect(() => {
    const element = screenRef.current;
    if (!element) return;
    let stopped = false;
    let showing = false;
    let sessionEnded = ended;
    let pendingInput = "";
    let pendingInputBytes = 0;
    let inputSending = false;
    let resizeActive = false;
    let latestResize: Dimensions | null = null;
    let observedPixelRatio = devicePixelRatio;
    let resumeRead: (() => void) | null = null;
    const terminal = new Terminal({
      scrollback: SCROLLBACK_LINES,
      fontSize: 12,
      lineHeight: 1.3,
    });
    // Theme tokens are read again whenever the system palette changes, so an
    // open terminal follows it. An unset token leaves xterm's own default.
    const applyTheme = () => {
      const tokens = getComputedStyle(document.documentElement);
      const token = (name: string) =>
        tokens.getPropertyValue(name).trim() || undefined;
      const text = token("--color-text-primary");
      const font = token("--font-family-mono");
      if (font) terminal.options.fontFamily = font;
      terminal.options.theme = {
        background: token("--color-canvas"),
        foreground: text,
        cursor: text,
        selectionBackground: token("--color-selection"),
      };
    };
    applyTheme();
    // xterm has no nonce option; its renderer and viewport create their
    // <style> elements inside open().
    withStyleNonce(element.ownerDocument, () => terminal.open(element));

    async function pollOutput(): Promise<void> {
      let cursor = "0";
      try {
        while (!stopped) {
          // Keep the reader and xterm screen across visibility changes. A pending
          // native read owns an exclusive lease and cannot be replaced on hide.
          if (!showing) {
            await new Promise<void>((resolve) => {
              resumeRead = resolve;
            });
            continue;
          }
          let page;
          try {
            page = await readUserTerminal(scope, session, cursor);
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
          await new Promise<void>((resolve) =>
            terminal.write(page.bytes, resolve),
          );
          cursor = page.nextCursor;
        }
      } catch (error) {
        if (!stopped) reportStatus(terminalErrorLabel(error));
      }
    }

    async function drainResizes(): Promise<void> {
      resizeActive = true;
      try {
        while (latestResize && !stopped) {
          const next = latestResize;
          latestResize = null;
          try {
            await resizeUserTerminal(scope, session, ...next);
          } catch (error) {
            if (!latestResize && !stopped)
              reportStatus(terminalErrorLabel(error));
          }
        }
      } finally {
        resizeActive = false;
      }
    }

    async function drainInput(): Promise<void> {
      inputSending = true;
      try {
        while (pendingInput && !stopped && !sessionEnded) {
          const chunk = pendingInput;
          pendingInput = "";
          pendingInputBytes = 0;
          try {
            const started = Date.now();
            for (
              let delay = INPUT_RETRY_FIRST_DELAY_MS;
              !stopped;
              delay = Math.min(delay * 2, INPUT_RETRY_MAX_DELAY_MS)
            ) {
              try {
                await writeUserTerminal(scope, session, chunk);
                break;
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
          } catch (error) {
            pendingInput = "";
            pendingInputBytes = 0;
            sessionEnded = terminalErrorCode(error) === "ended";
            if (!stopped) reportStatus(terminalErrorLabel(error), sessionEnded);
            break;
          }
        }
      } finally {
        inputSending = false;
      }
    }

    const inputListener = terminal.onData((data) => {
      if (stopped || sessionEnded || !data) return;
      const byteLength = utf8ByteLength(data);
      if (pendingInputBytes + byteLength > TERMINAL_INPUT_BYTES) {
        reportStatus(
          "Input was not sent: terminal input buffer limit reached.",
        );
        return;
      }
      pendingInput += data;
      pendingInputBytes += byteLength;
      if (!inputSending) void drainInput();
    });
    const refreshDimensions = () => {
      if (stopped || !showing) return;
      if (devicePixelRatio !== observedPixelRatio) {
        observedPixelRatio = devicePixelRatio;
        terminal.refresh(0, terminal.rows - 1);
      }
      const cell = (terminal as MeasuredTerminal)._core._renderService
        .dimensions.device.cell;
      if (cell.width <= 0 || cell.height <= 0) return;
      const scrollbarWidth =
        element.querySelector(".scrollbar.vertical")?.getBoundingClientRect()
          .width ?? 0;
      const next: Dimensions = [
        Math.max(
          MIN_COLUMNS,
          Math.min(
            TERMINAL_MAX_COLUMNS,
            Math.floor(
              ((element.clientWidth - scrollbarWidth) * devicePixelRatio) /
                cell.width,
            ),
          ),
        ),
        Math.max(
          MIN_ROWS,
          Math.min(
            TERMINAL_MAX_ROWS,
            Math.floor((element.clientHeight * devicePixelRatio) / cell.height),
          ),
        ),
      ];
      if (terminal.cols === next[0] && terminal.rows === next[1]) return;
      terminal.resize(...next);
      latestResize = next;
      if (!resizeActive) void drainResizes();
    };
    const palette = window.matchMedia?.(FORCED_COLORS_QUERY);
    // A new font face can change the cell size, so the grid is refitted.
    const followTheme = () => {
      applyTheme();
      refreshDimensions();
    };
    palette?.addEventListener("change", followTheme);
    const observer = new ResizeObserver(refreshDimensions);
    observer.observe(element);
    window.addEventListener("resize", refreshDimensions);
    displayRef.current = (show) => {
      showing = show;
      terminal.options.cursorBlink = show;
      if (!show) return;
      resumeRead?.();
      resumeRead = null;
      refreshDimensions();
      terminal.focus();
    };
    void pollOutput();
    return () => {
      stopped = true;
      resumeRead?.();
      displayRef.current = null;
      observer.disconnect();
      palette?.removeEventListener("change", followTheme);
      window.removeEventListener("resize", refreshDimensions);
      inputListener.dispose();
      terminal.dispose();
    };
  }, [ended, scope, session]);

  useEffect(() => {
    displayRef.current?.(visible);
  }, [visible, ended, scope, session]);

  return (
    <div className="terminal-dock__viewport" hidden={!visible}>
      <div className="terminal-dock__screen" ref={screenRef} />
    </div>
  );
}
