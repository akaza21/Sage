import { installNativeCommands } from "./fixtures/composerCommands";
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
  source: "harness";
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

async function openComposer(page: Page) {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await installNativeCommands(page, commands);
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
  // Resize/ResizeObserver positioning can land between browser calls. Read
  // both boxes together and wait for the menu to fit the new viewport too.
  await expect
    .poll(() =>
      listbox
        .getByRole("option")
        .nth(2)
        .evaluate((option) => {
          const row = option.getBoundingClientRect();
          const menu = option
            .closest(".composer-slash-menu")!
            .getBoundingClientRect();
          return {
            menuWithinViewport:
              menu.left >= 0 && menu.right <= window.innerWidth,
            rowWithinMenu: row.left >= menu.left && row.right <= menu.right + 1,
            // A long description is cut to one line inside the menu.
            singleLine: row.height < 40,
          };
        }),
    )
    .toEqual({
      menuWithinViewport: true,
      rowWithinMenu: true,
      singleLine: true,
    });
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

test("a /review that named its target shows the command and the target", async ({
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
    // As the host stores a `/review base main` picked from Codex's list.
    prompt.payload.text = "/review base main";
    turn.command = { name: "review", source: "builtin" };
    turn.sessionAction = {
      kind: "review",
      target: { kind: "baseBranch", branch: "main" },
    };
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
  await expect(bubble.locator(".acp-message__command")).toHaveText(
    "/review, Built-in",
  );
  await expect(bubble.locator(".acp-markdown")).toHaveText("base main");
  await expectNoViewportOverflow(page);
});

test("picking Codex's /review shows its input hint", async ({ page }) => {
  const field = await openComposer(page);
  await page.evaluate(() =>
    (
      window as unknown as { __emitCommandsChanged: (next: unknown) => void }
    ).__emitCommandsChanged([
      {
        name: "compact",
        description:
          "summarize conversation to prevent hitting the context limit",
        inputHint: null,
        source: "builtin",
      },
      {
        name: "review",
        description: "review my current changes and find issues",
        inputHint: "base <branch>, commit <sha>, or what to check",
        source: "builtin",
      },
      {
        name: "pdf",
        description: "Read PDFs",
        inputHint: null,
        source: "skill",
      },
    ]),
  );
  await field.fill("/rev");
  const menu = page.getByRole("listbox");
  await expect(menu.getByRole("option")).toHaveCount(1);
  await expect(menu.getByRole("option").first()).toContainText("/review");
  await field.press("Enter");
  await expect(field).toHaveValue("/review ");
  await expect(page.locator(".composer__command-hint")).toHaveText(
    "/review base <branch>, commit <sha>, or what to check",
  );
});

test("a typed terminal-only command offers copy, text and a focused terminal with unrun input", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  // The live catalog of a curated binding, with its terminal handoff; sends
  // are captured instead of reaching a harness.
  await page.evaluate(async () => {
    const [
      { acpCommandsDesktop },
      { acpDesktop },
      { useSessionAccessModeStore },
    ] = await Promise.all([
      import("/src/services/acpCommandsDesktop.ts"),
      import("/src/services/acpDesktop.ts"),
      import("/src/app/sessionAccessModeStore.ts"),
    ]);
    useSessionAccessModeStore.setState({ ready: true });
    const sent: string[] = [];
    Object.assign(window, { terminalSent: sent });
    acpCommandsDesktop.listCommands = async () => ({
      revision: 1,
      commands: [],
      terminalHandoff: {
        label: "Claude Code",
        command: 'claude --resume "4f1c2a"',
        terminalOnly: ["hooks"],
      },
    });
    acpDesktop.sendTurn = async (request) => {
      if (sent.length < 4) sent.push(request.prompt);
      throw new Error("captured");
    };
  });
  const composer = page.locator("form.composer").first();
  const field = composer.getByRole("combobox", { name: "Message agent" });
  await field.focus();
  await field.fill("/hooks");
  // The field's command controller is loaded once its menu shows.
  await expect(page.locator(".composer-slash-menu")).toBeVisible();
  await page.keyboard.press("Escape");
  await field.press("Enter");
  const held =
    "/hooks only runs in the terminal. Copy the command to resume this chat there, then type /hooks.";
  const choice = page.getByRole("group", { name: "Terminal-only command" });
  await expect(choice).toContainText(held);
  // The field's status region, mounted before the catch, announces it.
  await expect(
    page.locator(".composer__input-area > [role='status']"),
  ).toHaveText(held);
  await expect(
    choice.getByRole("button", { name: "Copy terminal command" }),
  ).toBeVisible();
  await expect(field).toHaveValue("/hooks");
  const sent = () =>
    page.evaluate(
      () => (window as unknown as { terminalSent: string[] }).terminalSent,
    );
  expect(await sent()).toEqual([]);
  await expectNoViewportOverflow(page);
  await expect(composer).toHaveScreenshot("composer-terminal-command.png");
  await choice.getByRole("button", { name: "Send as text anyway" }).click();
  await expect(choice).toHaveCount(0);
  await expect(field).toBeFocused();
  await expect.poll(sent).toEqual(["/hooks"]);
  // The simulator has no PTY. Substitute only native terminal I/O, retaining
  // the real dock controller, tab creation adapter, input adapter and xterm.
  await page.evaluate(async () => {
    const [{ useTerminalDockStore }, { useWorkbenchStore }] = await Promise.all(
      [import("/src/app/terminalDockStore.ts"), import("/src/app/store.ts")],
    );
    const input: string[] = [];
    Object.assign(window, { terminalInput: input });
    const sessionId = "f99cd56a-32d9-4600-a1f6-000000000001";
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: async (
          command: string,
          args?: { request: { text: string; cursor: string } },
        ) => {
          if (command === "get_terminal_dock_visible") return false;
          if (
            command === "set_terminal_dock_visible" ||
            command === "terminal_resize" ||
            command === "terminal_close"
          )
            return;
          if (command === "terminal_create")
            return { sessionId, revision: "1", shellPath: "simulated-shell" };
          if (command === "terminal_write") {
            if (input.length >= 4) throw new Error("Fixture input limit");
            input.push(args!.request.text);
            return;
          }
          if (command === "terminal_read") {
            if (args!.request.cursor !== "0")
              return new Promise(() => undefined);
            return {
              bytesBase64: btoa(`project> ${input.join("")}`),
              nextCursor: "1",
              truncated: false,
            };
          }
          throw new Error(`Unsupported terminal fixture command: ${command}`);
        },
      },
    });
    await useTerminalDockStore.getState().restore();
    useWorkbenchStore.setState((state) => ({
      sidebarOpen: !state.sidebarOpen,
    }));
  });
  await expect(
    page.getByRole("button", { name: "Toggle terminal", exact: true }),
  ).toBeVisible();
  await field.fill("/hooks");
  await field.press("Escape");
  await field.press("Enter");
  await choice.getByRole("button", { name: "Open in terminal" }).click();
  await expect(choice).toHaveCount(0);
  const dock = page.getByRole("region", { name: "Terminal", exact: true });
  await expect(dock).toBeVisible();
  await expect(dock.getByRole("tab")).toHaveCount(1);
  await expect(dock.locator(".xterm-rows")).toContainText(
    'project> claude --resume "4f1c2a"',
  );
  await expect(dock.locator(".xterm-helper-textarea")).toBeFocused();
  expect(
    await page.evaluate(
      () => (window as unknown as { terminalInput: string[] }).terminalInput,
    ),
  ).toEqual(['claude --resume "4f1c2a"']);
  expect(await sent()).toEqual(["/hooks"]);
});
