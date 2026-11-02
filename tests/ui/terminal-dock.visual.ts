import { expect, test, type Page } from "@playwright/test";
import {
  installScenario,
  openScenario,
} from "./shared/visualBrowserPrimitives";

/** The native boundary is simulated; the setting adapter, command and xterm are real. */
async function attachNativeTerminal(page: Page) {
  await page.evaluate(async () => {
    const [
      { getWorkbenchPorts },
      { nativeSettingsPort },
      { useTerminalDockStore },
      { useWorkbenchStore },
    ] = await Promise.all([
      import("/src/ports/workbenchRuntime.ts"),
      import("/src/ports/nativeSettingsPort.ts"),
      import("/src/workbench/workbenchLayoutStorage.ts"),
      import("/src/app/store.ts"),
    ]);
    let sequence = 0;
    const sessions = new Map<string, string>();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: async (
          command: string,
          args?: {
            visible: boolean;
            request: { handle: { sessionId: string }; cursor: string };
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
              const output = sessions.get(args!.request.handle.sessionId);
              if (!output) throw { code: "unknown_session" };
              if (args!.request.cursor !== "0")
                return new Promise(() => undefined);
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
            case "terminal_write":
              return;
            default:
              throw new Error(
                `Unsupported terminal fixture command: ${command}`,
              );
          }
        },
      },
    });
    Object.assign(getWorkbenchPorts().settings, {
      getTerminalDockVisible: nativeSettingsPort.getTerminalDockVisible,
      setTerminalDockVisible: nativeSettingsPort.setTerminalDockVisible,
    });
    await useTerminalDockStore.getState().restore();
    useWorkbenchStore.setState((state) => ({
      sidebarOpen: !state.sidebarOpen,
    }));
  });
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
  for (const visible of [true, false]) {
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-pressed", String(visible));
    await page.evaluate(async () => {
      const { useTerminalDockStore } =
        await import("/src/workbench/workbenchLayoutStorage.ts");
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

test("shortcut and Escape preserve sessions and return focus to the composer", async ({
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
