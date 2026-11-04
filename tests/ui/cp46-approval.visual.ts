import { expect, test, type Page } from "@playwright/test";

import {
  waitForAnimations,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

/** A running command gated by a pending Codex-shaped approval. */
async function applyApproval(page: Page, state: "received" | "declinePending") {
  await page.evaluate(async (state) => {
    const [{ useWorkbenchStore }, { acpProjectionStoreFor }] =
      await Promise.all([
        import("/src/app/store.ts"),
        import("/src/app/acpProjectionRegistry.ts"),
      ]);
    const store = acpProjectionStoreFor(
      useWorkbenchStore.getState().activeTabId,
    );
    const { page: current, scope } = store.getState();
    if (!current || !scope) throw new Error("Expected a loaded chat");
    const chatPage = structuredClone(current);
    const turn = chatPage.turns.at(-1)!;
    const prompt = turn.items[0];
    turn.state = "running";
    turn.terminalAt = null;
    turn.items = turn.items.filter(
      (item) =>
        item.id !== "approval-command" && item.id !== "approval-request",
    );
    turn.items.push(
      {
        ...prompt,
        id: "approval-command" as typeof prompt.id,
        ordinal: 4,
        kind: "toolCall",
        lifecycle: "streaming",
        toolState: "inProgress",
        terminalAt: null,
        payload: {
          presentation: {
            kind: "execute",
            label: "curl.exe",
            summary: null,
            cwd: null,
            exitCode: null,
            outputTail: null,
            outputTruncated: false,
            resources: [],
            activity: "ordinary",
          },
          inputSummary:
            'curl.exe -s -o NUL -w "%{http_code}" https://example.com',
        },
      } as never,
      {
        ...prompt,
        id: "approval-request" as typeof prompt.id,
        ordinal: 5,
        kind: "permissionRequest",
        lifecycle: "streaming",
        toolState: "waitingForApproval",
        terminalAt: null,
        payload: {
          requestId: "approval-1",
          state,
          label: "Permission request",
          summary: "Run a command",
          source: "harness",
          offeredOptionKinds: ["allow_once", "allow_always", "reject_once"],
          category: "execute",
          reason:
            "May I retry the network check outside the sandbox? The sandboxed request returned 000 without an HTTP response.",
        },
      } as never,
    );
    store.getState().applyAuthoritativePage(scope, chatPage);
  }, state);
}

for (const width of [480, 1280]) {
  test(`approval load failure keeps the toolbar reachable with a full queue and long draft at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 560 });
    await page.route(
      "**/src/features/chat/acp/PermissionPrompt.tsx*",
      (route) => route.abort("failed"),
    );
    await openScenario(page, "diff", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const { MAX_QUEUED_CHAT_PROMPTS } =
        await import("/src/domain/chatOwnership.ts");
      useWorkbenchStore.setState((state) => ({
        chatUiById: {
          ...state.chatUiById,
          [state.activeTabId!]: {
            ...state.chatUiById[state.activeTabId!],
            queuedPrompts: Array.from(
              { length: MAX_QUEUED_CHAT_PROMPTS },
              (_, index) => ({
                id: `queue-${index}`,
                text: `Follow-up ${index}`,
                attachedContext: [],
                bindingId: "fixture",
                modelId: null,
                status: "blocked" as const,
              }),
            ),
          },
        },
      }));
    });
    const draft = "Keep this draft while approval recovers.\n".repeat(40);
    const input = page.getByRole("textbox", { name: "Message agent" });
    await input.fill(draft);
    await applyApproval(page, "received");
    const dock = page.locator(".chat-dock");
    await expect(dock.getByRole("alert")).toContainText(
      "The approval could not load",
    );
    await expect(input).toHaveValue(draft);
    await expect(
      dock.getByLabel("Queued messages", { exact: true }),
    ).toBeVisible();
    await expect(
      dock.getByRole("region", { name: "Files edited in this chat" }),
    ).toBeVisible();
    expect(
      await dock
        .locator(".composer__body")
        .evaluate((body) => body.scrollHeight > body.clientHeight),
    ).toBe(true);
    const bounds = (await dock.boundingBox())!;
    const toolbar = dock.locator(".composer__toolbar");
    const row = (await toolbar.boundingBox())!;
    expect(row.y).toBeGreaterThanOrEqual(bounds.y);
    expect(row.y + row.height).toBeLessThanOrEqual(bounds.y + bounds.height);
    expect(row.x).toBeGreaterThanOrEqual(bounds.x);
    expect(row.x + row.width).toBeLessThanOrEqual(bounds.x + bounds.width);
    for (const control of [
      toolbar.getByRole("button", { name: "Harness", exact: true }),
      toolbar.getByRole("button", { name: "Access options", exact: true }),
      toolbar.getByRole("button", { name: "Cancel turn", exact: true }),
    ]) {
      const box = (await control.boundingBox())!;
      expect(box.y).toBeGreaterThanOrEqual(bounds.y);
      expect(box.y + box.height).toBeLessThanOrEqual(bounds.y + bounds.height);
    }
    await toolbar
      .getByRole("button", { name: "Cancel turn", exact: true })
      .click({ trial: true });
  });
}

test("edited files and saved diffs stay usable when approval replaces the composer", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "diff", "agent");
  await settleChatProjections(page);
  const summary = page.getByRole("region", {
    name: "Files edited in this chat",
  });
  await expect(summary).toBeVisible();
  await summary.getByRole("button", { name: /^1 file changed/ }).click();
  const viewChanges = page
    .getByRole("region", { name: "Edited files", exact: true })
    .getByRole("button", { name: "View changes" });
  await expect(viewChanges).toBeVisible();
  await expect(
    summary.getByRole("button", { name: /^Preview saved diff/ }),
  ).toBeVisible();

  await applyApproval(page, "received");
  const prompt = page.getByRole("group", {
    name: "Permission request",
    exact: true,
  });
  await expect(prompt).toBeVisible();
  await expect(page.locator("form.composer")).toBeHidden();
  await expect(summary).toBeVisible();
  const preview = summary.getByRole("button", { name: /^Preview saved diff/ });
  await preview.click();
  await expect(
    summary.getByRole("table", { name: "Inline saved diff" }),
  ).toBeVisible();
  await preview.click();
  await expect(viewChanges).toBeVisible();
  await viewChanges.click();
  await expect(
    page.getByRole("table", { name: "Unified file diff" }),
  ).toBeVisible();
  await expect(
    prompt.getByRole("button", { name: /Allow once/ }),
  ).toBeVisible();
  await expect(summary).toBeVisible();
});

test("CP-46 approval replaces the composer slot, opens its menu above, and yields back", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await openScenario(page, "waiting", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer");
  const before = await composer.boundingBox();
  await applyApproval(page, "received");

  // The composer-slot card; once settled, the transcript keeps its own
  // compact "Permission request" record (Codex s6/f0569 keeps the turn).
  const prompt = page
    .locator(".acp-permission-prompt")
    .and(page.getByRole("group", { name: "Permission request" }));
  await expect(prompt).toBeVisible();
  await expect(composer).toBeHidden();
  await expect(prompt.getByRole("button", { name: /Deny/ })).toBeVisible();
  // Codex s6/f0526: Deny reads in primary white, not muted.
  await expect(prompt.getByRole("button", { name: /Deny/ })).toHaveCSS(
    "color",
    "rgb(255, 255, 255)",
  );
  const allow = prompt.getByRole("button", { name: /Allow once/ });
  await expect(allow).toBeVisible();
  await expect(allow).toBeFocused();
  // The card shows the request's own text; the live command row is not
  // linked to the request on the wire, so it is never guessed.
  await expect(prompt.locator("pre")).toHaveText("Run a command");
  await expect(prompt.locator(".acp-permission-prompt__question")).toHaveText(
    "Allow this command?",
  );
  await expect(prompt.locator(".acp-permission-prompt__reason")).toContainText(
    "Agent's reason:",
  );
  // Stop stays for safety, as a quiet header control (s6/f0526 has none).
  const stop = prompt.getByRole("button", { name: "Stop turn" });
  await expect(stop).toBeVisible();
  await expect(prompt.locator("header")).toContainText("Stop");
  await expect(stop).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  // Allow once holds focus on arrival and shows Codex's 2 px ring: a 1 px
  // accent outline plus a 1 px accent spread (no 2 px outlines by policy).
  await expect(allow).toHaveCSS("outline-style", "solid");
  await expect(allow).toHaveCSS("outline-color", "rgb(58, 131, 247)");
  await expect(allow).toHaveCSS(
    "box-shadow",
    "rgb(58, 131, 247) 0px 0px 0px 1px",
  );
  // The running row keeps a spinner beside the Awaiting approval pill.
  await expect(
    page.locator(".project-chat__approval-spinner").first(),
  ).toBeVisible();
  const box = await prompt.boundingBox();
  expect(Math.abs(box!.width - before!.width)).toBeLessThanOrEqual(2);
  expect(Math.abs(box!.x - before!.x)).toBeLessThanOrEqual(2);

  const chevron = prompt.getByRole("button", { name: "More allow options" });
  await chevron.click();
  const menu = page.getByRole("menu", { name: "Allow options" });
  await expect(menu.getByRole("menuitem")).toHaveText([
    "Allow once",
    "Allow similar commands",
  ]);
  await waitForAnimations(menu);
  const menuBox = await menu.boundingBox();
  const chevronBox = await chevron.boundingBox();
  expect(menuBox!.y + menuBox!.height).toBeLessThanOrEqual(chevronBox!.y);
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(prompt).toBeVisible();

  const pill = page.getByText("Awaiting approval", { exact: true });
  await expect(pill.first()).toBeVisible();
  await expect(pill.first()).toHaveCSS("color", "rgb(64, 201, 119)");
  await expect(pill.first()).toHaveCSS("background-color", "rgb(49, 76, 60)");

  await applyApproval(page, "declinePending");
  await expect(prompt).toHaveCount(0);
  await expect(composer).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "Message agent" }),
  ).toBeVisible();
  await expect(
    page.getByText("Awaiting approval", { exact: true }),
  ).toHaveCount(0);
});

for (const width of [720, 480]) {
  test(`CP-46 approval card fills the composer slot at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await openScenario(page, "waiting", "agent");
    await settleChatProjections(page);
    const composer = page.locator("form.composer");
    await expect(composer).toBeVisible();
    const before = (await composer.boundingBox())!;
    await applyApproval(page, "received");
    const prompt = page
      .locator(".acp-permission-prompt")
      .and(page.getByRole("group", { name: "Permission request" }));
    await expect(prompt).toBeVisible();
    // The loading fallback shares the card's class and accessible group name.
    await expect(
      prompt.getByRole("button", { name: /Allow once/ }),
    ).toBeVisible();
    const box = (await prompt.boundingBox())!;
    expect(Math.abs(box.width - before.width)).toBeLessThanOrEqual(2);
    expect(Math.abs(box.x - before.x)).toBeLessThanOrEqual(2);
    // Deny and Allow keep to the right edge, wrapped or not.
    const actions = (await prompt
      .locator(".acp-permission-prompt__actions")
      .boundingBox())!;
    for (const button of [prompt.locator(".acp-permission-prompt__split")]) {
      const edge = (await button.boundingBox())!;
      expect(
        Math.abs(edge.x + edge.width - (actions.x + actions.width)),
      ).toBeLessThanOrEqual(1);
    }
    const deny = (await prompt
      .getByRole("button", { name: /Deny/ })
      .boundingBox())!;
    const split = (await prompt
      .locator(".acp-permission-prompt__split")
      .boundingBox())!;
    if (deny.y + 1 < split.y)
      expect(
        Math.abs(deny.x + deny.width - (actions.x + actions.width)),
      ).toBeLessThanOrEqual(1);
    expect(box.x + box.width).toBeLessThanOrEqual(width);
    if (width <= 600)
      await page
        .getByRole("button", { name: "Show projects and chats" })
        .click();
    const row = page
      .locator(".project-chat-row")
      .filter({ has: page.locator(".project-chat__approval-spinner") })
      .first();
    const chat = row.locator(".project-chat");
    await expect(row).toBeVisible();
    await page.mouse.move(0, 0);
    for (const state of ["resting", "hovered", "keyboard-focused"]) {
      await test.step(`${state} approval row keeps its content clear`, async () => {
        if (state === "hovered") await row.hover();
        if (state === "keyboard-focused") {
          await page.mouse.move(0, 0);
          await chat.focus();
          await page.keyboard.press("Shift+Tab");
          await page.keyboard.press("Tab");
          await expect(chat).toBeFocused();
          await expect(chat.locator(":scope:focus-visible")).toHaveCount(1);
        }
        const actions = row.locator(".project-chat__quick-actions");
        await expect(actions).toHaveCSS(
          "opacity",
          state === "resting" ? "0" : "1",
        );
        const geometry = await row.evaluate((element) => {
          const box = (selector: string) =>
            element.querySelector(selector)!.getBoundingClientRect();
          const content = box(".project-chat__copy");
          const title = box(".project-chat__title-label");
          const pill = box(".rail-pill--approval");
          const spinner = box(".project-chat__approval-spinner");
          const actions = box(".project-chat__quick-actions");
          return {
            titleToPill: pill.left - title.right,
            contentToSpinner: spinner.left - content.right,
            pillToSpinner: spinner.left - pill.right,
            contentToActions: actions.left - content.right,
          };
        });
        expect(geometry.titleToPill).toBeGreaterThanOrEqual(4);
        expect(geometry.contentToSpinner).toBeGreaterThanOrEqual(4);
        expect(geometry.pillToSpinner).toBeGreaterThanOrEqual(4);
        // Pin and Archive replace the spinner visually on hover and focus.
        if (state !== "resting")
          expect(geometry.contentToActions).toBeGreaterThanOrEqual(4);
      });
    }
  });
}
