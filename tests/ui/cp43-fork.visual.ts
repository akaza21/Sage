import { expect, test, type Page } from "@playwright/test";
import {
  waitForAnimations,
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";

test.use({ deviceScaleFactor: 1 });

async function openForkableAnswer(page: Page) {
  await page.setViewportSize({ width: 1872, height: 1080 });
  const workbench = await openScenario(page, "rich", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/transcriptSelectionFixtures.ts");
    fixture.installTranscriptSelectionFixture(false, true);
  });
  await settleTranscriptAtEnd(page);
  return workbench;
}

test("CP-43: Branch in new chat forks above the parent and links back", async ({
  page,
}) => {
  const workbench = await openForkableAnswer(page);
  const parentTitle = await page
    .locator(".agent-task-header__title")
    .first()
    .innerText();
  const more = page.getByRole("button", { name: "More actions", exact: true });
  await expect(more).toHaveAttribute("aria-haspopup", "menu");
  await more.click();
  const menu = page.getByRole("menu", { name: "Answer actions" });
  await expect(menu.getByRole("menuitem")).toHaveCount(1);
  const item = menu.getByRole("menuitem", { name: "Branch in new chat" });
  await expect(item).toBeVisible();
  await waitForAnimations(page.locator(".chat-transcript-actions").last());
  await waitForAnimations(menu);
  const menuBox = (await menu.boundingBox())!;
  const triggerBox = (await more.boundingBox())!;
  expect(menuBox.y + menuBox.height).toBeLessThanOrEqual(triggerBox.y);
  await expect(workbench).toHaveScreenshot("answer-more-menu.png");

  await item.click();
  const dialog = page.getByRole("dialog", { name: "Fork chat from here" });
  await expect(dialog).toBeVisible();
  const box = (await dialog.boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(
    Math.abs(box.x + box.width / 2 - viewport.width / 2),
  ).toBeLessThanOrEqual(4);
  expect(
    Math.abs(box.y + box.height / 2 - viewport.height / 2),
  ).toBeLessThanOrEqual(4);
  const options = dialog
    .getByRole("group", { name: "Fork chat from here" })
    .getByRole("button");
  await expect(options).toHaveCount(1);
  await expect(options.first()).toContainText("Fork in this workspace");
  await expect(options.first()).toContainText(
    "Fork from this message in the current workspace",
  );
  await expect(dialog.getByText(/worktree/i)).toHaveCount(0);
  // Codex s2/f0055: a #2a2a2a dialog whose focused first option shows no
  // fill or ring after a pointer open; hover fills it.
  await expect(dialog).toHaveCSS("background-color", "rgb(42, 42, 42)");
  await expect(options.first()).toBeFocused();
  await expect(options.first()).toHaveCSS(
    "background-color",
    "rgba(0, 0, 0, 0)",
  );
  await expect(options.first()).toHaveCSS("outline-style", "none");
  await expect(workbench).toHaveScreenshot("fork-dialog.png");
  await options.first().hover();
  await expect(options.first()).toHaveCSS(
    "background-color",
    "rgb(53, 53, 53)",
  );
  await page.mouse.move(0, 0);

  await page.keyboard.press("ArrowDown");
  await expect(options.first()).not.toHaveAttribute("data-fresh");
  await expect(options.first()).toHaveCSS("outline-style", "solid");
  await page.emulateMedia({ forcedColors: "active" });
  await expect(options.first()).toHaveCSS("outline-style", "solid");
  await expect(options.first()).toHaveCSS("outline-width", "2px");
  await page.emulateMedia({ forcedColors: "none" });

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await more.click();
  await item.click();
  await options.first().click();
  await expect(dialog).toBeHidden();
  await expect(page.locator(".agent-task-header__title").first()).toHaveText(
    `${parentTitle} (2)`,
  );
  // The sidebar lists the fork directly above its parent (Codex s2/f0063).
  const rows = page.getByRole("button", {
    name: new RegExp(`^Open chat ${parentTitle}`),
  });
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText(`${parentTitle} (2)`);
  await expect(rows.nth(1)).toHaveText(parentTitle);
  // Like Codex (s2/f0064-f0066) the fork takes over the current pane alone,
  // never paired with its parent: the parent's history up to the forked
  // answer sits read-only above the divider, and the fork has no forkable
  // answer of its own yet.
  const forkPane = page.getByRole("region", {
    name: `Chat pane ${parentTitle} (2)`,
    exact: true,
  });
  await expect(
    page.getByRole("region", { name: `Chat pane ${parentTitle}`, exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("separator", { name: "Resize chat panes" }),
  ).toHaveCount(0);
  const inherited = forkPane.getByRole("group", {
    name: "History from the parent chat",
  });
  await expect(inherited).toBeVisible();
  await expect(inherited).toContainText("The preceding explanation");
  const link = forkPane.getByRole("button", {
    name: "Continued from chat",
    exact: true,
  });
  await expect(link).toBeVisible();
  const historyBox = (await inherited.boundingBox())!;
  const linkBox = (await link.boundingBox())!;
  expect(historyBox.y + historyBox.height).toBeLessThanOrEqual(linkBox.y);
  await expect(workbench).toHaveScreenshot("fork-thread-divider.png");
  // The inherited answer keeps a usable "…", offering only Copy (s2/f0066).
  await inherited
    .getByRole("button", { name: "More actions", exact: true })
    .last()
    .click();
  await expect(
    page.getByRole("menu", { name: "Answer actions" }).getByRole("menuitem"),
  ).toHaveText(["Copy"]);
  await page.keyboard.press("Escape");

  await link.click();
  await expect(page.locator(".agent-task-header__title").first()).toHaveText(
    parentTitle,
  );
});
