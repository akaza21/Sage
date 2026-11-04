import { readFileSync } from "node:fs";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Terminal } from "@xterm/xterm";

const native = vi.hoisted(() => ({ read: vi.fn(), resize: vi.fn() }));

vi.mock(import("../services/userTerminalDesktop"), async (importOriginal) => ({
  ...(await importOriginal()),
  readUserTerminal: native.read,
  resizeUserTerminal: native.resize,
}));

import TerminalDock from "./TerminalDock";

const FORCED_COLORS = "(forced-colors: active)";
const OUTPUT = "sage terminal output";
const scope = {
  projectId: "project-1",
  projectGeneration: "2",
  chatId: "chat-1",
};
const handle = {
  sessionId: "f99cd56a-32d9-4600-a1f6-269e05734268",
  revision: "1",
  shellPath: "/bin/sh",
};

// jsdom applies a stylesheet's plain rules to computed style but never
// evaluates its @media rules, so the media query result is emulated here by
// applying the app stylesheet's own forced-colors rules while the stubbed
// query reports active.
const appStylesheet = readFileSync("src/styles/global.css", "utf8");
// The styles the running app gives the dock: xterm's own and the dock's.
const dockStylesheets = [
  "node_modules/@xterm/xterm/css/xterm.css",
  "src/workbench/terminal-dock.css",
].map((file) => readFileSync(file, "utf8"));
let forcedColors = false;
let forcedColorsRules: HTMLStyleElement | null = null;
const mediaListeners = new Map<string, Set<() => void>>();

function forcedColorsBlock(sheet: CSSStyleSheet): string {
  const block = Array.from(sheet.cssRules).find(
    (rule): rule is CSSMediaRule =>
      rule instanceof CSSMediaRule && rule.media.mediaText === FORCED_COLORS,
  );
  if (!block) throw new Error("global.css has no forced-colors block");
  return Array.from(block.cssRules, (rule) => rule.cssText).join("\n");
}

function setForcedColors(active: boolean, sheet: CSSStyleSheet) {
  forcedColors = active;
  forcedColorsRules?.remove();
  forcedColorsRules = null;
  if (active) {
    forcedColorsRules = document.createElement("style");
    forcedColorsRules.textContent = forcedColorsBlock(sheet);
    document.head.append(forcedColorsRules);
  }
  act(() => mediaListeners.get(FORCED_COLORS)?.forEach((listen) => listen()));
}

function stubMatchMedia() {
  mediaListeners.clear();
  vi.stubGlobal("matchMedia", (query: string) => {
    const listeners = mediaListeners.get(query) ?? new Set<() => void>();
    mediaListeners.set(query, listeners);
    return {
      media: query,
      get matches() {
        return query === FORCED_COLORS && forcedColors;
      },
      addEventListener: (_type: string, listener: () => void) =>
        listeners.add(listener),
      removeEventListener: (_type: string, listener: () => void) =>
        listeners.delete(listener),
      // xterm watches the device pixel ratio through the legacy API.
      addListener: (listener: () => void) => listeners.add(listener),
      removeListener: (listener: () => void) => listeners.delete(listener),
    };
  });
}

/** The live xterm the dock opened, observed without replacing it. */
let opened: ReturnType<typeof vi.spyOn>;

function mountedTerminal(): Terminal {
  expect(opened).toHaveBeenCalledTimes(1);
  return opened.mock.contexts[0] as Terminal;
}

/** A declaration from the stylesheet xterm's DOM renderer injected. */
function rendered(selector: string, property: string): string | undefined {
  const css = Array.from(
    document.querySelectorAll("style"),
    (style) => style.textContent ?? "",
  ).join("\n");
  const block = new RegExp(
    `${selector.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")} \\{([^}]*)\\}`,
    "u",
  ).exec(css)?.[1];
  return new RegExp(`(?:^|[;\\s])${property}: ([^;]+);`, "u")
    .exec(block ?? "")?.[1]
    ?.trim();
}

/**
 * The colour the viewport behind the rows computes to. jsdom keeps `var()`
 * unresolved in computed values and does not inherit custom properties, so a
 * token reference is resolved here from the root, where the theme defines it.
 */
function viewportBackground(container: HTMLElement): string {
  const viewport = container.querySelector(".xterm-viewport")!;
  const value = getComputedStyle(viewport).backgroundColor;
  const token = /^var\((--[\w-]+)\)$/u.exec(value)?.[1];
  return token
    ? getComputedStyle(document.documentElement).getPropertyValue(token).trim()
    : value;
}

function renderDock() {
  return render(
    <TerminalDock
      scope={scope}
      session={handle}
      ended={false}
      onStatus={vi.fn()}
    />,
  );
}

let theme: HTMLStyleElement;
let dockStyles: HTMLStyleElement[] = [];

beforeEach(() => {
  forcedColors = false;
  stubMatchMedia();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  theme = document.createElement("style");
  theme.textContent = appStylesheet;
  document.head.append(theme);
  dockStyles = dockStylesheets.map((css) => {
    const style = document.createElement("style");
    style.textContent = css;
    document.head.append(style);
    return style;
  });
  native.resize.mockResolvedValue(undefined);
  native.read.mockReset();
  native.read
    .mockResolvedValueOnce({
      bytes: new TextEncoder().encode(OUTPUT),
      nextCursor: "1",
      truncated: false,
    })
    .mockImplementation(() => new Promise(() => undefined));
  opened = vi.spyOn(Terminal.prototype, "open");
});

afterEach(() => {
  cleanup();
  forcedColorsRules?.remove();
  forcedColorsRules = null;
  theme.remove();
  dockStyles.forEach((style) => style.remove());
  opened.mockRestore();
  vi.unstubAllGlobals();
});

it("renders in the app theme's canvas, text, selection and mono font", async () => {
  const { container } = renderDock();
  const terminal = mountedTerminal();
  expect(terminal.options.theme).toEqual({
    background: "#141414",
    foreground: "#c5c5c5",
    cursor: "#c5c5c5",
    selectionBackground: "#264f78",
  });
  expect(rendered(".xterm-rows", "color")).toBe("#c5c5c5");
  expect(rendered(".xterm-selection div", "background-color")).toBe("#264f78");
  await waitFor(() =>
    expect(
      (container.querySelector(".xterm-scrollable-element") as HTMLElement)
        .style.backgroundColor,
    ).toBe("rgb(20, 20, 20)"),
  );
  // The part-row under the last whole row shows the viewport.
  expect(viewportBackground(container)).toBe("#141414");
  const faces = String(rendered(".xterm-rows", "font-family"))
    .split(",")
    .map((face) => face.trim());
  expect(faces).toEqual(
    String(terminal.options.fontFamily)
      .split(",")
      .map((face) => face.trim()),
  );
  // WebKit on macOS resolves a bare `monospace` to Courier.
  expect(faces.indexOf("Menlo")).toBeGreaterThanOrEqual(0);
  expect(faces.indexOf("Menlo")).toBeLessThan(faces.indexOf("monospace"));
});

it("follows the forced-colors palette while mounted and keeps its output", async () => {
  const { container, unmount } = renderDock();
  const terminal = mountedTerminal();
  await waitFor(() =>
    expect(terminal.buffer.active.getLine(0)?.translateToString(true)).toBe(
      OUTPUT,
    ),
  );
  const font = terminal.options.fontFamily;

  setForcedColors(true, theme.sheet!);
  expect(terminal.options.theme).toEqual({
    background: "Canvas",
    foreground: "CanvasText",
    cursor: "CanvasText",
    selectionBackground: "Highlight",
  });
  expect(terminal.options.fontFamily).toBe(font);
  expect(viewportBackground(container)).toBe("Canvas");
  // jsdom has no canvas to resolve system colours, so the renderer can only
  // show here that it stopped drawing the dark palette.
  expect(rendered(".xterm-rows", "color")).not.toBe("#c5c5c5");
  expect(rendered(".xterm-selection div", "background-color")).not.toBe(
    "#264f78",
  );

  setForcedColors(false, theme.sheet!);
  expect(rendered(".xterm-rows", "color")).toBe("#c5c5c5");
  expect(viewportBackground(container)).toBe("#141414");
  expect(
    (container.querySelector(".xterm-scrollable-element") as HTMLElement).style
      .backgroundColor,
  ).toBe("rgb(20, 20, 20)");
  expect(rendered(".xterm-selection div", "background-color")).toBe("#264f78");
  expect(opened).toHaveBeenCalledTimes(1);
  expect(terminal.buffer.active.getLine(0)?.translateToString(true)).toBe(
    OUTPUT,
  );

  unmount();
  expect(mediaListeners.get(FORCED_COLORS)?.size).toBe(0);
});
