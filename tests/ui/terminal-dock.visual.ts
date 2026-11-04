import { expect, test, type Page } from "@playwright/test";
import {
  installScenario,
  openScenario,
} from "./shared/visualBrowserPrimitives";

type TerminalActivity = {
  polling: boolean;
  reads: number;
  resizes: number;
  minColumns: number;
  minRows: number;
  output: string;
};

/** The native boundary is simulated; the setting adapter, command and xterm are real. */
async function attachNativeTerminal(page: Page, waitForControls = true) {
  await page.evaluate(async () => {
    const [{ useTerminalDockStore }, { useWorkbenchStore }] = await Promise.all(
      [import("/src/app/terminalDockStore.ts"), import("/src/app/store.ts")],
    );
    let sequence = 0;
    const sessions = new Map<string, string>();
    const pty = { columns: 80, rows: 24 };
    const input: number[] = [];
    const activity: TerminalActivity = {
      polling: false,
      reads: 0,
      resizes: 0,
      minColumns: 500,
      minRows: 300,
      output: "",
    };
    Object.assign(window, {
      terminalPtySize: pty,
      terminalPtyInput: input,
      terminalActivity: activity,
    });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: async (
          command: string,
          args?: {
            visible: boolean;
            request: {
              handle: { sessionId: string };
              cursor: string;
              columns: number;
              rows: number;
              text: string;
            };
          },
        ) => {
          if (
            command === "get_terminal_dock_visible" ||
            command === "set_terminal_dock_visible"
          ) {
            return (
              window as unknown as {
                nativeDockSetting: (value?: boolean) => Promise<boolean>;
              }
            ).nativeDockSetting(
              command.startsWith("set_") ? args!.visible : undefined,
            );
          }
          switch (command) {
            case "terminal_create": {
              const sessionId = `f99cd56a-32d9-4600-a1f6-${String(++sequence).padStart(12, "0")}`;
              sessions.set(sessionId, `Retained shell ${sequence}\r\n`);
              return {
                sessionId,
                revision: "1",
                shellPath: `shell-${sequence}`,
              };
            }
            case "terminal_read": {
              activity.reads += 1;
              const output = sessions.get(args!.request.handle.sessionId);
              if (!output) throw { code: "unknown_session" };
              if (args!.request.cursor !== "0") {
                if (!activity.polling) return new Promise(() => undefined);
                await new Promise((resolve) => setTimeout(resolve, 50));
                const bytes = activity.output;
                activity.output = "";
                return {
                  bytesBase64: btoa(bytes),
                  nextCursor: String(BigInt(args!.request.cursor) + 1n),
                  truncated: false,
                };
              }
              return {
                bytesBase64: btoa(output),
                nextCursor: "1",
                truncations: "0",
                truncated: false,
              };
            }
            case "terminal_close":
              sessions.delete(args!.request.handle.sessionId);
              return;
            case "terminal_resize":
              activity.resizes += 1;
              activity.minColumns = Math.min(
                activity.minColumns,
                args!.request.columns,
              );
              activity.minRows = Math.min(activity.minRows, args!.request.rows);
              Object.assign(pty, {
                columns: args!.request.columns,
                rows: args!.request.rows,
              });
              return;
            case "terminal_write":
              for (const byte of new TextEncoder().encode(args!.request.text)) {
                if (input.length >= 32)
                  throw new Error("Terminal fixture input limit reached");
                input.push(byte);
              }
              return;
            default:
              throw new Error(
                `Unsupported terminal fixture command: ${command}`,
              );
          }
        },
      },
    });
    await useTerminalDockStore.getState().restore();
    useWorkbenchStore.setState((state) => ({
      sidebarOpen: !state.sidebarOpen,
    }));
  });
  if (waitForControls)
    await expect(
      page.getByRole("button", { name: "Toggle terminal", exact: true }),
    ).toBeVisible();
}

test.use({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });

test.beforeEach(async ({ page }) => {
  let persisted = false;
  // Outlives the WebView, as the native settings service does.
  await page.exposeFunction("nativeDockSetting", (value?: boolean) => {
    if (typeof value === "boolean") persisted = value;
    return persisted;
  });
  await openScenario(page, "agent-single", "agent");
  await attachNativeTerminal(page);
});

test("Settings suspends terminal polling and rendering without shrinking or replacing its session", async ({
  page,
}) => {
  await page.evaluate(() => {
    (
      window as unknown as { terminalActivity: TerminalActivity }
    ).terminalActivity.polling = true;
  });
  await page
    .getByRole("button", { name: "Toggle terminal", exact: true })
    .click();
  const dock = page.getByRole("region", {
    name: "Terminal",
    exact: true,
    includeHidden: true,
  });
  const rows = dock.locator(".xterm-rows");
  await expect(rows).toContainText("Retained shell 1");
  const original = await gridGeometry(page);
  expect(original.columns).toBeGreaterThan(2);
  expect(original.rows).toBeGreaterThan(1);
  await page.clock.pauseAt(new Date("2026-08-31T09:31:00.000Z"));
  const activity = () =>
    page.evaluate(() => {
      const { reads, resizes, minColumns, minRows } = (
        window as unknown as { terminalActivity: TerminalActivity }
      ).terminalActivity;
      return { reads, resizes, minColumns, minRows };
    });
  const before = await activity();
  await page.keyboard.press("Control+,");
  await expect(
    page.getByRole("dialog", { name: "Agent settings", exact: true }),
  ).toBeVisible();
  await expect(dock).toBeHidden();
  await page.evaluate(() => {
    (
      window as unknown as { terminalActivity: TerminalActivity }
    ).terminalActivity.output = "Output received while hidden\r\n";
  });
  // Finish the outstanding native read, then observe another polling interval.
  await page.clock.runFor(250);
  const hidden = await activity();
  await page.clock.runFor(250);
  expect(await activity()).toEqual(hidden);
  expect(hidden.reads).toBe(before.reads);
  expect(hidden.resizes).toBe(before.resizes);
  expect(hidden.minColumns).toBeGreaterThanOrEqual(original.columns);
  expect(hidden.minRows).toBeGreaterThanOrEqual(original.rows);
  await expect(rows).not.toContainText("Output received while hidden");

  await page
    .getByRole("button", { name: "Close settings", exact: true })
    .click();
  await page.clock.runFor(250);
  await expect(dock).toBeVisible();
  await expect(rows).toContainText("Output received while hidden");
  await expect(rows).toContainText("Retained shell 1");
  await expect(dock.getByRole("tab")).toHaveCount(1);
  expect((await activity()).reads).toBeGreaterThan(hidden.reads);
  await expect.poll(() => gridGeometry(page)).toEqual(original);
  expect((await activity()).resizes).toBe(hidden.resizes + 1);
});

test("defaults closed; the labelled toggle persists open and closed across reload", async ({
  page,
}) => {
  const toggle = page.getByRole("button", {
    name: "Toggle terminal",
    exact: true,
  });
  const dock = page.getByRole("region", { name: "Terminal", exact: true });
  await expect(toggle).toHaveText(">_ Terminal");
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await expect(toggle).toHaveAttribute("title", "Show terminal (Ctrl+`)");
  await expect(dock).toHaveCount(0);
  const layout = await toggle.evaluate((button) => {
    const icon = button.querySelector("span")!.getBoundingClientRect();
    const label = [...button.childNodes].find(
      (node) =>
        node.nodeType === Node.TEXT_NODE &&
        node.textContent?.includes("Terminal"),
    )!;
    const range = document.createRange();
    range.selectNodeContents(label);
    const text = range.getBoundingClientRect();
    const footer = button.closest("footer")!.getBoundingClientRect();
    return {
      icon: { x: icon.x, y: icon.y, width: icon.width, height: icon.height },
      text: { x: text.x, y: text.y, height: text.height },
      footer: { top: footer.top, bottom: footer.bottom },
    };
  });
  expect(layout.text.x).toBeGreaterThanOrEqual(
    layout.icon.x + layout.icon.width,
  );
  expect(
    Math.abs(
      layout.icon.y +
        layout.icon.height / 2 -
        layout.text.y -
        layout.text.height / 2,
    ),
  ).toBeLessThan(2);
  expect(layout.text.y).toBeGreaterThanOrEqual(layout.footer.top);
  expect(layout.text.y + layout.text.height).toBeLessThanOrEqual(
    layout.footer.bottom,
  );
  for (const visible of [true, false]) {
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-pressed", String(visible));
    await page.evaluate(async () => {
      const { useTerminalDockStore } =
        await import("/src/app/terminalDockStore.ts");
      await useTerminalDockStore.getState().flush();
    });
    await page.reload();
    await installScenario(page, "agent-single");
    await attachNativeTerminal(page);
    await expect(toggle).toHaveAttribute("aria-pressed", String(visible));
    if (visible) await expect(dock).toBeVisible();
    else await expect(dock).toBeHidden();
  }
});

test("shortcut and chrome Escape preserve sessions and return focus to the composer", async ({
  page,
}) => {
  const composer = page.getByRole("textbox", {
    name: "Message agent",
    exact: true,
  });
  const toggle = page.getByRole("button", {
    name: "Toggle terminal",
    exact: true,
  });
  const dock = page.getByRole("region", { name: "Terminal", exact: true });
  await composer.focus();
  await page.keyboard.press("Control+Backquote");
  await expect(dock.locator(".xterm-helper-textarea:visible")).toBeFocused();
  await expect(dock.locator(".xterm-rows:visible")).toContainText(
    "Retained shell 1",
  );
  await dock.getByRole("button", { name: "New terminal", exact: true }).click();
  await expect(dock.locator(".xterm-rows:visible")).toContainText(
    "Retained shell 2",
  );
  await dock.getByRole("button", { name: "New terminal", exact: true }).focus();
  await page.keyboard.press("Escape");
  await expect(dock).toBeHidden();
  await expect(composer).toBeFocused();
  await toggle.click();
  await expect(dock.getByRole("tab")).toHaveCount(2);
  await expect(dock.locator(".xterm-helper-textarea:visible")).toBeFocused();
  await expect(dock.locator(".xterm-rows:visible")).toContainText(
    "Retained shell 2",
  );
  await dock.getByRole("tab").first().click();
  await expect(dock.locator(".xterm-rows:visible")).toContainText(
    "Retained shell 1",
  );
  await page.keyboard.press("Control+Backquote");
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await expect(composer).toBeFocused();
});

test("navigation and terminal commands stay independent across layout changes", async ({
  page,
}) => {
  const toggle = page.getByRole("button", {
    name: "Toggle terminal",
    exact: true,
  });
  const dock = page.getByRole("region", { name: "Terminal", exact: true });
  const navigation = page.getByRole("button", {
    name: "Hide projects and chats",
  });
  await toggle.click();
  await expect(dock.locator(".xterm-helper-textarea:visible")).toBeFocused();
  await page
    .getByRole("textbox", { name: "Message agent", exact: true })
    .focus();
  await page.keyboard.press("ControlOrMeta+b");
  await expect(navigation).toBeHidden();
  await expect(dock).toBeVisible();
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "Show projects and chats" }).click();
  await expect(navigation).toBeVisible();
  await expect(dock).toBeVisible();
  await page.keyboard.press("Control+Backquote");
  await expect(dock).toBeHidden();
  await page.keyboard.press("ControlOrMeta+b");
  await expect(navigation).toBeHidden();
  await toggle.click();
  await expect(dock.locator(".xterm-rows:visible")).toContainText(
    "Retained shell 1",
  );
  await expect(navigation).toBeHidden();
  await expect(dock.getByRole("tab")).toHaveCount(1);
});

test("reopening after ending the last session starts a focused usable shell", async ({
  page,
}) => {
  const toggle = page.getByRole("button", {
    name: "Toggle terminal",
    exact: true,
  });
  const dock = page.getByRole("region", { name: "Terminal", exact: true });
  await toggle.click();
  await expect(dock.locator(".xterm-rows:visible")).toContainText(
    "Retained shell 1",
  );
  await dock
    .getByRole("button", { name: "Close terminal tab", exact: true })
    .click();
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await toggle.click();
  await expect(dock.locator(".xterm-rows:visible")).toContainText(
    "Retained shell 2",
  );
  await expect(dock.locator(".xterm-helper-textarea:visible")).toBeFocused();
});

test("returns to the toggle when opened without a focus target", async ({
  page,
}) => {
  await page.evaluate(() =>
    (document.activeElement as HTMLElement | null)?.blur(),
  );
  const toggle = page.getByRole("button", {
    name: "Toggle terminal",
    exact: true,
  });
  await toggle.click();
  await expect(page.locator(".xterm-helper-textarea:visible")).toBeFocused();
  await page.getByRole("button", { name: "New terminal", exact: true }).focus();
  await page.keyboard.press("Escape");
  await expect(toggle).toBeFocused();
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
});

for (const mode of ["Agent", "Editor"]) {
  test(`Escape reaches terminal input before and after toggling in ${mode} mode`, async ({
    page,
  }) => {
    await page
      .getByRole("radiogroup", { name: "Window mode" })
      .locator("label")
      .filter({ hasText: mode })
      .click();
    const dock = page.getByRole("region", { name: "Terminal", exact: true });
    const toggle = page.getByRole("button", {
      name: "Toggle terminal",
      exact: true,
    });
    const input = dock.locator(".xterm-helper-textarea:visible");
    const received: number[] = [];
    await toggle.click();
    for (let attempt = 0; attempt < 2; attempt += 1) {
      await expect(input).toBeFocused();
      await page.keyboard.press("Escape");
      received.push(27);
      await expect
        .poll(() =>
          page.evaluate(
            () =>
              (window as unknown as { terminalPtyInput: number[] })
                .terminalPtyInput,
          ),
        )
        .toEqual(received);
      await expect(dock).toBeVisible();
      await expect(input).toBeFocused();
      await page.keyboard.press("Control+Backquote");
      await expect(dock).toBeHidden();
      await toggle.click();
      await expect(dock.getByRole("tab")).toHaveCount(1);
    }
  });
}

async function gridGeometry(page: Page) {
  return page.locator(".terminal-dock__screen").evaluate((host) => {
    const grid = host.querySelector<HTMLElement>(".xterm-screen")!;
    const rows = host.querySelector<HTMLElement>(".xterm-rows")!;
    const scrollbar = host.querySelector(".scrollbar.vertical")!;
    const font = getComputedStyle(rows);
    const context = new OffscreenCanvas(1, 1).getContext("2d")!;
    context.font = `${font.fontSize} ${font.fontFamily}`;
    const character = context.measureText("W");
    const pty = (
      window as unknown as {
        terminalPtySize: { columns: number; rows: number };
      }
    ).terminalPtySize;
    return {
      ...pty,
      renderedRows: rows.children.length,
      width: grid.getBoundingClientRect().width,
      height: grid.getBoundingClientRect().height,
      availableWidth:
        host.clientWidth - scrollbar.getBoundingClientRect().width,
      availableHeight: host.clientHeight,
      cellWidth: character.width,
      cellHeight: Number.parseFloat(
        getComputedStyle(rows.firstElementChild!).height,
      ),
    };
  });
}

async function dockGeometry(page: Page) {
  return page.locator(".terminal-dock").evaluate((dock) => {
    const header = dock.querySelector<HTMLElement>(".terminal-dock__header")!;
    const viewport = dock.querySelector<HTMLElement>(
      ".terminal-dock__viewport",
    )!;
    const pty = (
      window as unknown as {
        terminalPtySize: { columns: number; rows: number };
      }
    ).terminalPtySize;
    return {
      unusedHeight: Math.round(
        dock.clientHeight -
          header.getBoundingClientRect().height -
          viewport.getBoundingClientRect().height,
      ),
      columns: pty.columns,
      rows: pty.rows,
    };
  });
}

test("the terminal viewport follows the dock as the window shrinks and grows", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page
    .getByRole("button", { name: "Toggle terminal", exact: true })
    .click();
  await expect(page.locator(".xterm-screen:visible")).toBeVisible();
  await expect
    .poll(async () => (await dockGeometry(page)).unusedHeight)
    .toBe(0);
  const first = await dockGeometry(page);
  let tallest = first;
  for (const [width, height] of [
    [900, 620],
    [1700, 1000],
    [1280, 800],
  ]) {
    await page.setViewportSize({ width, height });
    await expect
      .poll(async () => (await dockGeometry(page)).unusedHeight)
      .toBe(0);
    if (height === 1000) tallest = await dockGeometry(page);
  }
  expect(tallest.rows).toBeGreaterThan(first.rows);
  await expect.poll(() => dockGeometry(page)).toEqual(first);
});

for (const deviceScaleFactor of [1, 1.25, 2]) {
  test.describe(`terminal grid at ${deviceScaleFactor} device scale`, () => {
    test.use({ deviceScaleFactor });

    test("fits both axes and matches the PTY after narrow and wide resizes", async ({
      page,
    }) => {
      await page
        .getByRole("button", { name: "Toggle terminal", exact: true })
        .click();
      await expect(page.locator(".xterm-screen:visible")).toBeVisible();
      const host = page.locator(".terminal-dock__screen");
      let firstFit: Awaited<ReturnType<typeof gridGeometry>> | undefined;
      for (const [width, height] of [
        [32, 32],
        [800, 160],
        [1600, 320],
        [800, 160],
      ]) {
        await host.evaluate(
          (element, size) => {
            element.style.width = `${size.width}px`;
            element.style.height = `${size.height}px`;
          },
          { width, height },
        );
        await expect
          .poll(async () => {
            const grid = await gridGeometry(page);
            return {
              fitsWidth: grid.width <= grid.availableWidth,
              fitsHeight: grid.height <= grid.availableHeight,
              fillsWidth: grid.availableWidth - grid.width < grid.cellWidth,
              fillsHeight: grid.availableHeight - grid.height < grid.cellHeight,
              matchesColumns:
                grid.width === Math.round(grid.columns * grid.cellWidth),
              matchesRows: grid.renderedRows === grid.rows,
            };
          })
          .toEqual({
            fitsWidth: true,
            fitsHeight: true,
            fillsWidth: true,
            fillsHeight: true,
            matchesColumns: true,
            matchesRows: true,
          });
        if (width === 800) {
          const fit = await gridGeometry(page);
          if (firstFit) expect(fit).toEqual(firstFit);
          else firstFit = fit;
        }
      }
    });
  });
}

test("reports a terminal controller loading failure", async ({ page }) => {
  await page.route("**/src/workbench/terminalDockCommands.tsx*", (route) =>
    route.abort(),
  );
  await page.reload();
  await installScenario(page, "agent-single");
  await attachNativeTerminal(page, false);
  await expect(
    page.getByRole("status").filter({ hasText: "Terminal failed to load." }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Toggle terminal", exact: true }),
  ).toHaveCount(0);
});
