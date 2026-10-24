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
  await expect(right).toContainText(
    "Side chats are temporary and disappear when you close the app.",
  );
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
  await expect(right.locator(".agent-chat-pane__title")).toHaveText(prompt);
  await expect(
    right.locator(".agent-chat-pane__identity .spinner"),
  ).toBeVisible();
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
  await expect(
    right.locator(".agent-chat-pane__identity .spinner"),
  ).toBeHidden();
  await expect(workbench).toHaveScreenshot("side-chat-completed.png");
  await right
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Close side chat?" });
  await expect(
    dialog.getByRole("button", { name: "Close side chat", exact: true }),
  ).toBeFocused();
  await expect(workbench).toHaveScreenshot("side-chat-close-confirm.png");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await right
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
  await right.locator("textarea").fill("A running temporary question");
  await right
    .getByRole("button", { name: "Send message", exact: true })
    .click();
  await expect(
    right.locator(".agent-chat-pane__identity .spinner"),
  ).toBeVisible();
  await right
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Close side chat?" });
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  await dialog
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
});
