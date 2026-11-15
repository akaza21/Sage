import { expect, test } from "@playwright/test";
import {
  expectNoViewportOverflow,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";
import {
  dragAnswerPhrase,
  openSelectionScenario,
} from "./shared/transcriptSelection";

test.use({ deviceScaleFactor: 1 });

test("AP-A1/A2: resize, resource collapse, and a new Chat never add an unsolicited pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  await openScenario(page, "agent-dual", "agent");
  await settleChatProjections(page);
  const panes = page.locator(".agent-chat-pane");
  const visibleIds = () =>
    panes.evaluateAll((elements) =>
      elements.map((element) => element.getAttribute("data-chat-id")),
    );
  const original = await visibleIds();
  expect(original).toEqual(["visual-chat-primary"]);
  for (const centerWidth of [859, 860, 1200, 859, 1200]) {
    // The 326 px navigation plus its 8 px splitter sit beside the center.
    await page.setViewportSize({ width: centerWidth + 334, height: 1000 });
    await expect
      .poll(async () =>
        Math.round((await page.locator(".center-region").boundingBox())!.width),
      )
      .toBe(centerWidth);
    expect(await visibleIds()).toEqual(original);
  }
  await page.getByRole("button", { name: /^Show files and preview/ }).click();
  await expect
    .poll(
      async () => (await page.locator(".center-region").boundingBox())!.width,
    )
    .toBeLessThan(860);
  expect(await visibleIds()).toEqual(original);
  await page
    .getByRole("button", { name: "Collapse files, changes, and preview" })
    .click();
  await expect
    .poll(
      async () => (await page.locator(".center-region").boundingBox())!.width,
    )
    .toBeGreaterThanOrEqual(860);
  expect(await visibleIds()).toEqual(original);
  const chatRows = page.locator(".project-chat");
  const listedIds = () =>
    chatRows.evaluateAll((rows) =>
      [...new Set(rows.map((row) => row.getAttribute("data-chat-id")))].sort(),
    );
  const originalChats = await listedIds();
  await page.evaluate(async () => {
    const { installWindowNewChatFixture } =
      await import("/tests/ui/fixtures/agentWindowFixtures.ts");
    const { acpDesktop } = await import("/src/services/acpDesktop.ts");
    const { session } = await import("/tests/ui/fixtures/visualFixtureIds.ts");
    const { turn } = await import("/tests/ui/fixtures/visualChatPages.ts");
    const { textItem } =
      await import("/tests/ui/fixtures/visualItemBuilders.ts");
    installWindowNewChatFixture();
    const getChatPage = acpDesktop.getChatPage;
    let current: Awaited<ReturnType<typeof getChatPage>> | null = null;
    acpDesktop.getChatPage = async (request, decodeGoal) =>
      current?.chatId === request.chatId
        ? structuredClone(current)
        : getChatPage(request, decodeGoal);
    acpDesktop.startSession = async (request) => {
      const initial = await acpDesktop.getChatPage({
        ...request,
        cursor: null,
      });
      const started = {
        ...session(),
        bindingId: request.bindingId,
        modelId: request.modelId,
      };
      current = {
        ...initial,
        currentSessionId: started.id,
        sessions: [started],
        projectionRevision: String(
          Number(initial.projectionRevision) + 1,
        ) as typeof initial.projectionRevision,
      };
      return { projection: structuredClone(current) };
    };
    acpDesktop.sendTurn = async (request) => {
      const initial = await acpDesktop.getChatPage({
        ...request,
        cursor: null,
      });
      const sent = turn(request.chatId, initial.turns.length, "completed", []);
      sent.items = [
        textItem(
          {
            chatId: request.chatId,
            turnId: sent.id,
            ordinal: 0,
            createdAt: sent.createdAt,
          },
          "user",
          request.prompt,
        ),
      ];
      current = {
        ...initial,
        turns: [...initial.turns, sent],
        projectionRevision: String(
          Number(initial.projectionRevision) + 1,
        ) as typeof initial.projectionRevision,
      };
      return { projection: structuredClone(current) };
    };
  });
  await page
    .locator(".project-sidebar__actions")
    .getByRole("button", { name: "New chat", exact: true })
    .click();
  const draft = page.getByRole("region", { name: "New chat draft" });
  await expect(
    draft.getByRole("heading", { name: "What should we work on?" }),
  ).toBeVisible();
  const composer = draft.getByRole("combobox", { name: "Message agent" });
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue("");
  await expect(panes).toHaveCount(0);
  expect(await listedIds()).toEqual(originalChats);
  await expect(chatRows.and(page.locator('[aria-current="page"]'))).toHaveCount(
    0,
  );
  await page.setViewportSize({ width: 1888, height: 1000 });
  await expect(panes).toHaveCount(0);
  await composer.fill("Start one chat from the unsaved draft");
  await expect(
    draft.getByRole("button", { name: "Send message", exact: true }),
  ).toBeEnabled();
  await composer.press("Enter");
  await expect(draft).toHaveCount(0);
  await expect(panes).toHaveCount(1);
  await expect(panes.first()).toHaveAttribute("data-chat-id", "ap-a-chat-1");
  await expect
    .poll(async () => (await listedIds()).length)
    .toBe(originalChats.length + 1);
  await expect(
    panes.getByText("Start one chat from the unsaved draft", { exact: true }),
  ).toHaveCount(1);
  await expect(
    panes.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("");
  await expect(panes.getByRole("alert")).toHaveCount(0);
  await page.setViewportSize({ width: 1193, height: 1000 });
  await expect(panes).toHaveCount(1);
  await page.setViewportSize({ width: 1888, height: 1000 });
  await expect(panes).toHaveCount(1);
});

test("AP-A1/A3: only an explicit side-chat action opens a second titled, closable pane", async ({
  page,
}) => {
  await openSelectionScenario(page);
  const panes = page.locator(".agent-chat-pane");
  await expect(panes).toHaveCount(1);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  await expect(panes).toHaveCount(2);
  const header = page.locator(".agent-chat-center__header");
  await expect(header.locator(".agent-task-header__title")).toHaveCount(1);
  await expect(header.locator(".side-chat-tab")).toHaveCount(1);
  await expect(
    header.getByRole("button", { name: "Close side chat", exact: true }),
  ).toBeVisible();
  const sideId = await panes.last().getAttribute("data-chat-id");
  // A 859 px center (below the two-pane minimum) beside the navigation.
  await page.setViewportSize({ width: 1193, height: 1000 });
  await expect(panes).toHaveCount(1);
  await expect(panes.first()).toHaveAttribute("data-chat-id", sideId!);
  await page.setViewportSize({ width: 1888, height: 1080 });
  await expect(panes).toHaveCount(1);
  await page.keyboard.press("Control+Alt+s");
  await expect(panes).toHaveCount(2);
  await expect(panes.last()).toHaveAttribute("data-chat-id", sideId!);
  await expect(header.locator(".side-chat-tab")).toHaveCount(1);
  await expect(
    header.getByRole("button", { name: "Close side chat", exact: true }),
  ).toBeVisible();
  await header
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Close side chat?" });
  await expect(dialog).toBeVisible();
  await dialog
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await expect(panes).toHaveCount(1);
  await expect(panes.first()).not.toHaveAttribute("data-chat-id", sideId!);
});

test("AP-A4: Escape closes Preview's innermost menu before the resource pane", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/browser-address.html?artifact-focus");
  const pane = page.getByLabel("Agent artifacts", { exact: true });
  await expect(pane).toBeVisible();
  const trigger = pane.getByRole("button", {
    name: "More options",
    exact: true,
  });
  await trigger.click();
  const menu = page.getByRole("menu", { name: "Browser menu" });
  await expect(menu).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(pane).toBeVisible();
  await expect(trigger).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(pane).toBeHidden();
});

for (const harness of ["codex", "claude-code", "pi"]) {
  test(`AP-A5: ${harness} sidebar hides to its rail and exposes header New chat`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1080 });
    const workbench = await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async (harnessId) => {
      const { installChatChromeFixture } =
        await import("/tests/ui/fixtures/chatChromeFixture.ts");
      installChatChromeFixture(harnessId, false);
    }, harness);
    const navigation = page.locator(".project-sidebar");
    await page
      .getByRole("button", { name: "Hide projects and chats" })
      .press("Enter");
    await expect
      .poll(async () => {
        const bounds = await navigation.boundingBox();
        return bounds ? Math.round(bounds.width) : null;
      })
      .toBe(46);
    await expect(
      navigation.locator(".project-sidebar__icon-rail"),
    ).toBeVisible();
    await expect(navigation.locator(".project-sidebar__actions")).toHaveCount(
      0,
    );
    await expect(navigation.locator(".project-sidebar__scroll")).toHaveCount(0);
    await expect(navigation.locator(".project-sidebar__account")).toHaveCount(
      0,
    );
    await expect(
      page
        .locator(".agent-task-header")
        .getByRole("button", { name: "New chat", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(workbench).toHaveScreenshot(
      `agent-sidebar-hidden-${harness}.png`,
    );
    await page
      .getByRole("button", { name: "Show projects and chats" })
      .press("Enter");
    await expect(navigation.locator(".project-sidebar__scroll")).toBeVisible();
    await expect(
      page
        .locator(".agent-task-header")
        .getByRole("button", { name: "New chat", exact: true }),
    ).toHaveCount(0);
    await expect(workbench).toHaveScreenshot(
      `agent-sidebar-shown-${harness}.png`,
    );
  });
}

test("AP-A5: collapsed rail Go to projects reopens and focuses a folded Projects", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1080 });
  await openScenario(page, "agent-single", "agent");
  const target = page.getByRole("region", { name: "Projects", exact: true });
  const disclosure = target.getByRole("button", {
    name: "Projects",
    exact: true,
  });
  await disclosure.click();
  await expect(disclosure).toHaveAttribute("aria-expanded", "false");
  await page.getByRole("button", { name: "Hide projects and chats" }).click();
  await expect(page.locator(".project-sidebar__scroll")).toHaveCount(0);
  await page
    .getByRole("button", { name: "Go to projects", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Hide projects and chats" }),
  ).toHaveAttribute("aria-expanded", "true");
  await expect(target).toBeVisible();
  await expect(disclosure).toBeFocused();
  await expect(disclosure).toHaveAttribute("aria-expanded", "true");
  await expectNoViewportOverflow(page);
});

for (const [control, organize] of [
  ["Go to recent chats", "project"],
  // In one list there is no Projects section; Recents stands in.
  ["Go to projects", "list"],
] as const) {
  test(`AP-A5: collapsed rail ${control} focuses Recents (${organize})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1080 });
    // agent-dual has an unpinned chat, so Recents has a row to list.
    await openScenario(page, "agent-dual", "agent");
    await page.evaluate(async (mode) => {
      const { setAgentRailOrganize } =
        await import("/src/workbench/agentRailViewPrefs.ts");
      setAgentRailOrganize(mode);
    }, organize);
    await page.getByRole("button", { name: "Hide projects and chats" }).click();
    await expect(page.locator(".project-sidebar__scroll")).toHaveCount(0);
    await page.getByRole("button", { name: control, exact: true }).click();
    const target = page.getByRole("region", { name: "Recents", exact: true });
    await expect(target).toBeVisible();
    await expect(
      target.getByRole("heading", { name: "Recents" }),
    ).toBeFocused();
    await expectNoViewportOverflow(page);
  });
}

test("AP-A6: a fresh profile opens Agent and an explicit Editor preference survives reload", async ({
  page,
}) => {
  await page.goto("/");
  await page.evaluate(() =>
    localStorage.removeItem("sage.workbench.layout.v1"),
  );
  await page.reload();
  await expect(
    page.getByRole("radio", { name: "Agent", exact: true }),
  ).toBeChecked();
  await page
    .getByRole("radio", { name: "Editor", exact: true })
    .locator("..")
    .click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          JSON.parse(localStorage.getItem("sage.workbench.layout.v1") ?? "{}")
            .windowMode,
      ),
    )
    .toBe("editor");
  await page.reload();
  await expect(
    page.getByRole("radio", { name: "Editor", exact: true }),
  ).toBeChecked();
});
