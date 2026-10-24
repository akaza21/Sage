import { expect, test } from "@playwright/test";
import {
  dragAnswerPhrase,
  openSelectionScenario,
} from "./shared/transcriptSelection";

test.use({ deviceScaleFactor: 1 });
test("CP-41: create, comment, inspect, edit, send and retain read-only annotations", async ({
  page,
}) => {
  const workbench = await openSelectionScenario(page);
  const selected = await dragAnswerPhrase(page);
  await page.getByRole("button", { name: "Add to chat", exact: true }).click();
  const comment = page.getByRole("textbox", {
    name: "Annotation comment",
    exact: true,
  });
  await expect(comment).toBeFocused();
  await expect(comment).toHaveAttribute(
    "placeholder",
    "Add an optional comment\u2026",
  );
  await expect(
    page.getByRole("button", { name: "Annotation 1", exact: true }),
  ).toBeVisible();
  const badge = (await page
    .getByRole("button", { name: "Annotation 1", exact: true })
    .boundingBox())!;
  expect(
    Math.abs(badge.x + badge.width / 2 - selected.x - selected.width),
  ).toBeLessThanOrEqual(4);
  expect(selected.y - badge.y).toBeGreaterThanOrEqual(22);
  expect(selected.y - badge.y).toBeLessThanOrEqual(30);
  await expect(workbench).toHaveScreenshot("annotation-field.png");
  await comment.fill("Is this true for null?");
  await comment.press("Enter");
  await expect(comment).toBeHidden();
  await expect(workbench).toHaveScreenshot("annotation-badge.png");
  const chip = page.getByRole("button", { name: "1 annotation", exact: true });
  await expect(chip).toBeVisible();
  await expect(workbench).toHaveScreenshot("annotation-chip.png");
  await chip.hover();
  const card = page.getByRole("dialog", { name: "Annotations", exact: true });
  await expect(card).toContainText("selected phrase");
  await expect(card).toContainText("Is this true for null?");
  await expect(workbench).toHaveScreenshot("annotation-chip-hover.png");
  await page.getByRole("button", { name: "Edit annotation 1" }).click();
  await expect(comment).toHaveValue("Is this true for null?");
  await comment.press("Enter");
  await page.locator(".composer textarea").fill("Explain this annotation.");
  await page
    .locator(".composer")
    .getByRole("button", { name: "Send message", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Annotation 1", exact: true }),
  ).toBeHidden();
  await expect(page.locator(".annotation-chip--sent")).toContainText(
    "1 annotation",
  );
  const sentChip = (await page
    .locator(".annotation-chip--sent")
    .boundingBox())!;
  const bubble = (await page
    .locator(".acp-message--user")
    .last()
    .boundingBox())!;
  expect(bubble.y - sentChip.y - sentChip.height).toBeGreaterThanOrEqual(6);
  expect(bubble.y - sentChip.y - sentChip.height).toBeLessThanOrEqual(10);
  await page.locator(".annotation-chip--sent button").hover();
  await expect(card).toContainText("Is this true for null?");
  await expect(
    page.getByRole("button", { name: "Edit annotation 1" }),
  ).toBeHidden();
  await expect(workbench).toHaveScreenshot("annotation-sent.png");
});

test("CP-41: Escape keeps no-comment annotation; deletion clears the source badge", async ({
  page,
}) => {
  await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page.getByRole("button", { name: "Add to chat", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Annotation comment", exact: true })
    .press("Escape");
  await page.getByRole("button", { name: "1 annotation", exact: true }).hover();
  await page.getByRole("button", { name: "Delete annotation 1" }).click();
  await expect(page.locator(".composer__annotation-row")).toBeHidden();
  await expect(
    page.getByRole("button", { name: "Annotation 1", exact: true }),
  ).toBeHidden();
});

test("CP-41: cap is visible, deletion renumbers and annotations alone can send", async ({
  page,
}) => {
  await openSelectionScenario(page);
  for (const phrase of [
    "The",
    "preceding",
    "explanation",
    "introduces",
    "selected phrase",
    "and",
    "its",
    "behavior",
  ]) {
    await dragAnswerPhrase(page, phrase);
    await page
      .getByRole("button", { name: "Add to chat", exact: true })
      .click();
    await page
      .getByRole("textbox", { name: "Annotation comment", exact: true })
      .press("Escape");
  }
  await expect(
    page.getByRole("button", { name: "8 annotations", exact: true }),
  ).toBeVisible();
  await dragAnswerPhrase(page, "null");
  await page.getByRole("button", { name: "Add to chat", exact: true }).click();
  await expect(
    page
      .getByRole("status")
      .filter({ hasText: "Up to 8 annotations per message" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "8 annotations", exact: true })
    .hover();
  await page
    .getByRole("button", { name: "Delete annotation 4", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "7 annotations", exact: true }),
  ).toBeVisible();
  const fourth = page.locator(".annotation-card__entry").nth(3);
  await expect(fourth).toContainText("selected phrase");
  await expect(fourth.locator(".annotation-card__number")).toHaveText("4.");
  await page.locator(".composer textarea").focus();
  await expect(page.locator(".composer textarea")).toHaveValue("");
  await page
    .locator(".composer")
    .getByRole("button", { name: "Send message", exact: true })
    .click();
  await expect(page.locator(".annotation-chip--sent")).toContainText(
    "7 annotations",
  );
  await expect(page.locator(".transcript-annotation-badge")).toHaveCount(0);
});
