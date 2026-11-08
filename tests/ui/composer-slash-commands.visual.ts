import { expect, test, type Page } from "@playwright/test";
import {
  expectNoViewportOverflow,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

type Command = { name: string; description: string; inputHint: string | null };

const commands: Command[] = [
  { name: "compact", description: "Compact the conversation", inputHint: null },
  { name: "review", description: "Review a pull request", inputHint: "PR" },
  { name: "mcp:docs:search", description: "Search docs", inputHint: "query" },
];

/**
 * A desktop runtime stand-in: list_acp_commands answers with the current
 * list, and the change event reaches the listener the menu installs.
 */
async function installNativeCommands(page: Page) {
  await page.evaluate((initial) => {
    const callbacks = new Map<number, (event: unknown) => void>();
    const state = {
      revision: 1,
      commands: initial,
      requests: [] as unknown[],
      listeners: [] as { event: string; handler: number }[],
    };
    let nextId = 1;
    Object.assign(window, {
      __slashCommands: state,
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
    });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: (
          command: string,
          args: { request?: unknown; event?: string; handler?: number },
        ) => {
          if (command === "list_acp_commands") {
            state.requests.push(args.request);
            return Promise.resolve({
              revision: state.revision,
              commands: state.commands,
            });
          }
          if (command === "plugin:event|listen") {
            state.listeners.push({
              event: args.event!,
              handler: args.handler!,
            });
            return Promise.resolve(state.listeners.length);
          }
          if (command === "plugin:event|unlisten") return Promise.resolve();
          return Promise.reject(new Error("visual fixture"));
        },
        transformCallback: (callback: (event: unknown) => void) => {
          callbacks.set(nextId, callback);
          return nextId++;
        },
        unregisterCallback: (id: number) => callbacks.delete(id),
      },
    });
    Object.assign(window, {
      __emitCommandsChanged: (next: typeof initial) => {
        const request = state.requests.at(-1) as {
          projectId: string;
          chatId: string;
          sessionId: string;
          runtimeEpoch: string;
        };
        state.revision += 1;
        state.commands = next;
        for (const listener of state.listeners)
          if (listener.event === "acp-commands-changed-v1")
            callbacks.get(listener.handler)?.({
              event: listener.event,
              id: listener.handler,
              payload: {
                schemaVersion: 1,
                sageProjectId: request.projectId,
                sageChatId: request.chatId,
                sageAgentSessionId: request.sessionId,
                runtimeEpoch: request.runtimeEpoch,
                revision: state.revision,
              },
            });
      },
    });
  }, commands);
}

async function openComposer(page: Page) {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await installNativeCommands(page);
  const field = page
    .locator("form.composer")
    .first()
    .getByRole("textbox", { name: "Message agent" });
  await field.focus();
  return field;
}

test("typing / lists, filters and inserts the session's commands from the keyboard", async ({
  page,
}) => {
  const field = await openComposer(page);
  await page.keyboard.type("/");
  const listbox = page.getByRole("listbox", { name: "Slash commands" });
  await expect(listbox.getByRole("option")).toHaveCount(3);
  await expect(field).toBeFocused();
  await expect(field).toHaveAttribute("aria-controls", /commands$/u);
  const activeOption = async () =>
    page.evaluate(() => {
      const field = document.activeElement as HTMLElement;
      const id = field.getAttribute("aria-activedescendant");
      return id
        ? document.getElementById(id)?.getAttribute("aria-label")
        : null;
    });
  expect(await activeOption()).toBe("/compact, Compact the conversation");
  await page.keyboard.press("ArrowDown");
  expect(await activeOption()).toBe(
    "/review, Review a pull request, Input: PR",
  );

  // The popover opens above the composer and stays inside the window.
  const composerBox = (await page
    .locator("form.composer")
    .first()
    .boundingBox())!;
  const menuBox = (await page.locator(".composer-slash-menu").boundingBox())!;
  expect(menuBox.y + menuBox.height).toBeLessThanOrEqual(composerBox.y + 1);
  await expectNoViewportOverflow(page);

  await page.keyboard.type("se");
  await expect(listbox.getByRole("option")).toHaveCount(1);
  await page.keyboard.press("Enter");
  await expect(field).toHaveValue("/mcp:docs:search ");
  await expect(listbox).toHaveCount(0);
  await expect(page.locator(".composer__command-hint")).toHaveText(
    "/mcp:docs:search query",
  );
  await page.keyboard.type("x");
  await expect(page.locator(".composer__command-hint")).toHaveCount(0);

  await field.fill("");
  await page.keyboard.type("/co");
  await expect(listbox.getByRole("option")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(listbox).toHaveCount(0);
  await expect(field).toHaveValue("/co");
  await expect(field).toBeFocused();
});

test("the open menu follows a new command list from the session", async ({
  page,
}) => {
  await openComposer(page);
  await page.keyboard.type("/");
  const listbox = page.getByRole("listbox", { name: "Slash commands" });
  await expect(listbox.getByRole("option")).toHaveCount(3);
  await page.evaluate(() =>
    (
      window as unknown as { __emitCommandsChanged(next: Command[]): void }
    ).__emitCommandsChanged([]),
  );
  await expect(listbox.getByRole("option")).toHaveCount(0);
  await expect(
    page.getByText("This harness advertises no slash commands."),
  ).toBeVisible();
  await page.evaluate(
    (next) =>
      (
        window as unknown as { __emitCommandsChanged(next: Command[]): void }
      ).__emitCommandsChanged(next),
    [{ name: "init", description: "Set up the project", inputHint: null }],
  );
  await expect(listbox.getByRole("option")).toHaveText([
    "/initSet up the project",
  ]);
});
