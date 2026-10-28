import { expect, test } from "@playwright/test";
import {
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
    await page.setViewportSize({ width: centerWidth + 393, height: 1000 });
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
  await page.evaluate(async () => {
    const { installWindowNewChatFixture } =
      await import("/tests/ui/fixtures/agentWindowFixtures.ts");
    installWindowNewChatFixture();
  });
  await page
    .locator(".project-sidebar__actions")
    .getByRole("button", { name: "New task", exact: true })
    .click();
  await expect(panes).toHaveCount(1);
  await expect(panes.first()).toHaveAttribute("data-chat-id", "ap-a-chat-1");
  await expect(
    panes.getByRole("textbox", { name: "Message agent" }),
  ).toBeVisible();
  await expect(panes.getByRole("alert")).toHaveCount(0);
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
  await page.setViewportSize({ width: 1252, height: 1000 });
  await expect(panes).toHaveCount(1);
  await expect(panes.first()).toHaveAttribute("data-chat-id", sideId!);
  await page.setViewportSize({ width: 1888, height: 1080 });
  await expect(panes).toHaveCount(1);
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
      .poll(async () => Math.round((await navigation.boundingBox())!.width))
      .toBe(54);
    await expect(
      navigation.locator(".project-sidebar__icon-rail"),
    ).toBeVisible();
    await expect(navigation.locator(".project-sidebar__actions")).toBeHidden();
    await expect(navigation.locator(".project-sidebar__scroll")).toBeHidden();
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
