import { expect, test, type Page } from "@playwright/test";
import {
  expectNoViewportOverflow,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

type Command = {
  name: string;
  description: string;
  inputHint: string | null;
  source: string;
};

const commands: Command[] = [
  {
    name: "compact",
    description: "Compact the conversation",
    inputHint: null,
    source: "harness",
  },
  {
    name: "review",
    description: "Review a pull request",
    inputHint: "PR",
    source: "harness",
  },
  {
    name: "mcp:docs:search",
    description: "Search docs",
    inputHint: "query",
    source: "harness",
  },
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
    .getByRole("combobox", { name: "Message agent" });
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

  // A pick is an ordinary edit: Undo brings back what was typed.
  await field.fill("");
  await page.keyboard.type("/rev");
  await expect(listbox.getByRole("option")).toHaveCount(1);
  await page.keyboard.press("Tab");
  await expect(field).toHaveValue("/review ");
  await page.keyboard.press("Control+z");
  await expect(field).toHaveValue("/rev");
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
    page
      .locator(".composer-slash-menu")
      .getByText("This harness advertises no slash commands."),
  ).toBeVisible();
  // The field's own status region announces it too.
  await expect(
    page.locator(".composer__input-area > [role='status']"),
  ).toHaveText("This harness advertises no slash commands.");
  await page.evaluate(
    (next) =>
      (
        window as unknown as { __emitCommandsChanged(next: Command[]): void }
      ).__emitCommandsChanged(next),
    [
      {
        name: "init",
        description: "Set up the project",
        inputHint: null,
        source: "harness",
      },
    ],
  );
  await expect(listbox.getByRole("option")).toHaveText([
    "/initSet up the project",
  ]);
});

test("an unfiltered menu sections commands by source and fits a narrow window", async ({
  page,
}) => {
  await openComposer(page);
  await page.evaluate(
    (next) =>
      (
        window as unknown as { __emitCommandsChanged(next: Command[]): void }
      ).__emitCommandsChanged(next),
    [
      {
        name: "deploy",
        description: "Ship the current branch to staging",
        inputHint: "environment",
        source: "harness",
      },
      {
        name: "frontend-design",
        description:
          "Create distinctive, production-grade frontend interfaces with high design quality that avoid generic looks",
        inputHint: null,
        source: "skill",
      },
      {
        name: "compact",
        description: "Clear conversation history but keep a summary in context",
        inputHint: "instructions",
        source: "builtin",
      },
      {
        name: "review",
        description: "Review a pull request",
        inputHint: null,
        source: "builtin",
      },
    ],
  );
  await page.keyboard.type("/");
  const listbox = page.getByRole("listbox", { name: "Slash commands" });
  const groups = listbox.getByRole("group");
  await expect(groups).toHaveCount(3);
  await expect(
    listbox.getByRole("group", { name: "Commands", exact: true }),
  ).toBeVisible();
  await expect(
    listbox.getByRole("group", { name: "Skills" }).getByRole("option"),
  ).toHaveText([/^\/frontend-design/u]);
  await expect(listbox.getByRole("option").first()).toHaveAccessibleName(
    "/compact, Clear conversation history but keep a summary in context, Input: instructions",
  );
  await page.setViewportSize({ width: 720, height: 900 });
  await expectNoViewportOverflow(page);
  const row = (await listbox.getByRole("option").nth(2).boundingBox())!;
  const menu = (await page.locator(".composer-slash-menu").boundingBox())!;
  // A long description is cut to one line inside the menu.
  expect(row.x + row.width).toBeLessThanOrEqual(menu.x + menu.width + 1);
  expect(row.height).toBeLessThan(40);
});

test("a prompt that ran a listed command shows the command beside what was typed", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 900 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const [{ acpProjectionStoreFor }, ids] = await Promise.all([
      import("/src/app/acpProjectionRegistry.ts"),
      import("/tests/ui/fixtures/visualFixtureIds.ts"),
    ]);
    const chatId = document.querySelector<HTMLElement>(
      ".acp-chat-projection[data-chat-id]",
    )!.dataset.chatId as never;
    const store = acpProjectionStoreFor(chatId);
    const chatPage = structuredClone(store.getState().page!);
    const turn = chatPage.turns.reduce((a, b) =>
      b.ordinal > a.ordinal ? b : a,
    );
    const prompt = turn.items.find(
      (item) => item.kind === "text" && item.payload.role === "user",
    );
    if (prompt?.kind !== "text") throw new Error("Expected a prompt");
    prompt.payload.text =
      "/release-notes summarise the changes since the last tag for the team";
    turn.command = { name: "release-notes", source: "skill" };
    const next = <T extends string>(revision: T) =>
      String(BigInt(revision) + 1n) as T;
    turn.revision = next(turn.revision);
    prompt.revision = next(prompt.revision);
    chatPage.projectionRevision = next(chatPage.projectionRevision);
    const applied = store.getState().applyAuthoritativePage(
      {
        projectId: ids.PROJECT_ID,
        projectGeneration: ids.GENERATION,
        chatId,
      },
      chatPage,
    );
    if (applied !== "accepted") throw new Error(applied);
  });
  const bubble = page.locator(".acp-message--user").last();
  const command = bubble.locator(".acp-message__command");
  await expect(command).toHaveText("/release-notes, Skill");
  await expect(bubble.locator(".acp-markdown")).toHaveText(
    "summarise the changes since the last tag for the team",
  );
  // The chip and its label share one line, above the input.
  const [name, source, input] = await Promise.all([
    command.locator(".acp-message__command-name").boundingBox(),
    command.locator(".acp-message__command-source").boundingBox(),
    bubble.locator(".acp-markdown").boundingBox(),
  ]);
  expect(source!.y).toBeLessThan(name!.y + name!.height);
  expect(source!.x).toBeGreaterThan(name!.x + name!.width);
  expect(input!.y).toBeGreaterThanOrEqual(name!.y + name!.height);
  await expectNoViewportOverflow(page);
});
