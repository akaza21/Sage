import { expect, test } from "@playwright/test";
import {
  dragAnswerPhrase,
  openSelectionScenario,
} from "./shared/transcriptSelection";

test.use({ deviceScaleFactor: 1 });
test("CP-42: temporary seeded right split, close confirmation, persisted opt-out", async ({
  page,
}) => {
  const workbench = await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  const right = page.locator(".agent-chat-pane").nth(1);
  const sideTab = page.locator(".agent-chat-center__side-tab");
  await expect(right).toContainText(
    "Side chats are temporary and disappear when you close the app.",
  );
  // The parent keeps its own header, title and rail selection; the side
  // pane's tab shares the header row; no project strip above its composer.
  const header = page.locator(".agent-chat-center__header");
  await expect(header).toHaveClass(/agent-chat-center__header--split/);
  const headerBox = (await header.boundingBox())!;
  const tabBox = (await sideTab.boundingBox())!;
  const rightBox = (await right.boundingBox())!;
  expect(tabBox.y).toBeGreaterThanOrEqual(headerBox.y);
  expect(tabBox.y + tabBox.height).toBeLessThanOrEqual(
    headerBox.y + headerBox.height + 1,
  );
  expect(Math.abs(tabBox.x - rightBox.x)).toBeLessThanOrEqual(12);
  await expect(header.locator(".agent-chat-center__side-tab")).toHaveCount(1);
  const current = page.locator("[aria-current='page'][title^='Open chat']");
  await expect(current).toHaveCount(1);
  await expect(current).not.toHaveAttribute("title", /Side chat/);
  await expect(right.locator(".chat-home__scope")).toHaveCount(0);
  const homeIcon = (await right.locator(".side-chat-home svg").boundingBox())!;
  expect(homeIcon.width).toBe(20);
  await expect(
    right.getByRole("button", { name: "1 selection", exact: true }),
  ).toBeVisible();
  await expect(right.locator("textarea")).toBeFocused();
  await expect(
    page.getByRole("separator", { name: "Resize chat panes" }),
  ).toHaveAttribute("aria-valuenow", "43");
  await expect(workbench).toHaveScreenshot("side-chat-empty.png");
  const prompt = "Explain the selection in this temporary discussion.";
  await right.locator("textarea").fill(prompt);
  await right
    .getByRole("button", { name: "Send message", exact: true })
    .click();
  await expect(sideTab.locator(".agent-chat-pane__title")).toHaveText(prompt);
  await expect(sideTab.locator(".spinner")).toBeVisible();
  await expect(page.locator(".project-chat__working")).toBeVisible();
  await expect(workbench).toHaveScreenshot("side-chat-running.png");
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/transcriptSelectionFixtures.ts");
    fixture.completeSideChatFixture();
  });
  await expect(right).toContainText(
    "This is a temporary discussion of the selected phrase.",
  );
  await expect(
    right.getByRole("button", { name: "More actions", exact: true }),
  ).toBeHidden();
  await expect(sideTab.locator(".spinner")).toBeHidden();
  await expect(workbench).toHaveScreenshot("side-chat-completed.png");
  await sideTab
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Close side chat?" });
  await expect(
    dialog.getByRole("button", { name: "Close side chat", exact: true }),
  ).toBeFocused();
  const dialogBox = (await dialog.boundingBox())!;
  expect(dialogBox.height).toBeGreaterThanOrEqual(164);
  expect(dialogBox.height).toBeLessThanOrEqual(176);
  const checkbox = (await dialog
    .getByRole("checkbox", { name: "Don't ask again" })
    .boundingBox())!;
  expect(checkbox.width).toBe(14);
  await expect(dialog.getByRole("button", { name: "Cancel" })).toHaveCSS(
    "background-color",
    "rgb(53, 53, 53)",
  );
  await expect(workbench).toHaveScreenshot("side-chat-close-confirm.png");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await sideTab
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await dialog.getByRole("checkbox", { name: "Don't ask again" }).check();
  await dialog
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  await page
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
  await expect(dialog).toBeHidden();
});

test("CP-42: a running temporary chat closes only after confirmation", async ({
  page,
}) => {
  await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  const right = page.locator(".agent-chat-pane").nth(1);
  const sideTab = page.locator(".agent-chat-center__side-tab");
  await right.locator("textarea").fill("A running temporary question");
  await right
    .getByRole("button", { name: "Send message", exact: true })
    .click();
  await expect(sideTab.locator(".spinner")).toBeVisible();
  await sideTab
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Close side chat?" });
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  await dialog
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
});
