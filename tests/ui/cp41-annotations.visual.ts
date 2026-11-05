import { expect, test } from "@playwright/test";
import {
  dragAnswerPhrase,
  openSelectionScenario,
} from "./shared/transcriptSelection";

test.use({ deviceScaleFactor: 1 });

test("CP-41: annotation selection waits for deferred Markdown text", async ({
  page,
}) => {
  await page.route("**/ChatMarkdown.tsx*", async (route) => {
    // Simulate a cold renderer import that outlasts the fixture's layout frames.
    await new Promise<void>((resolve) => setTimeout(resolve, 1500));
    await route.continue();
  });
  await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page.getByRole("button", { name: "Add to chat", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Annotation comment", exact: true }),
  ).toBeFocused();
  await expect(
    page.getByRole("button", { name: "Annotation 1", exact: true }),
  ).toBeVisible();
});
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
  // Codex s2/f0095, f0113: the badge sits above the range end, its bottom
  // on the highlight top, so it never covers the annotated text.
  expect(Math.abs(badge.y + badge.height - selected.y)).toBeLessThanOrEqual(2);
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

for (const width of [720, 480]) {
  test(`CP-41: the comment field opens above the line, clear of the badge, at ${width}`, async ({
    page,
  }) => {
    await openSelectionScenario(page);
    await page.setViewportSize({ width, height: 1080 });
    const selected = await dragAnswerPhrase(page);
    await page
      .getByRole("button", { name: "Add to chat", exact: true })
      .click();
    const field = (await page.locator(".annotation-comment").boundingBox())!;
    const badge = (await page
      .getByRole("button", { name: "Annotation 1", exact: true })
      .boundingBox())!;
    // Codex s2/f0095: above the selection, so the rest of the answer and
    // its action row stay visible; below is only a fallback.
    expect(field.y + field.height).toBeLessThanOrEqual(selected.y - 4);
    const overlaps =
      field.x < badge.x + badge.width &&
      field.x + field.width > badge.x &&
      field.y < badge.y + badge.height &&
      field.y + field.height > badge.y;
    expect(overlaps).toBe(false);
    expect(field.x + field.width).toBeLessThanOrEqual(width);
  });
}

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
  // Each badge sits above its range end (Codex s2/f0113), so
  // the phrases skip a word to leave the next one's start uncovered.
  for (const phrase of [
    "The",
    "explanation",
    "second",
    "provides",
    "selected phrase",
    "selection",
    "behavior",
    "review",
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

for (const width of [1872, 720, 480]) {
  test(`CP-41: the open comment field leaves its badge visible at ${width}`, async ({
    page,
  }) => {
    await openSelectionScenario(page);
    await page.setViewportSize({ width, height: 1080 });
    await dragAnswerPhrase(page);
    await page
      .getByRole("button", { name: "Add to chat", exact: true })
      .click();
    const field = page.getByRole("form", { name: "Annotation comment" });
    const badge = page.getByRole("button", {
      name: "Annotation 1",
      exact: true,
    });
    const fieldBox = (await field.boundingBox())!;
    const badgeBox = (await badge.boundingBox())!;
    // Codex s2/f0095: field and badge are both visible, never stacked.
    const overlaps =
      fieldBox.x < badgeBox.x + badgeBox.width &&
      fieldBox.x + fieldBox.width > badgeBox.x &&
      fieldBox.y < badgeBox.y + badgeBox.height &&
      fieldBox.y + fieldBox.height > badgeBox.y;
    expect(overlaps).toBe(false);
    expect(fieldBox.width).toBeLessThanOrEqual(270);
    expect(fieldBox.x).toBeGreaterThanOrEqual(0);
    expect(fieldBox.x + fieldBox.width).toBeLessThanOrEqual(width);
    const save = (await field
      .getByRole("button", { name: "Save annotation" })
      .boundingBox())!;
    expect(save.x + save.width).toBeLessThanOrEqual(width);
    // The badge chip icon is muted and outlined, not white (s2/f0113).
    await field.getByRole("textbox").press("Enter");
    await expect(page.locator(".annotation-chip svg").first()).toHaveCSS(
      "color",
      "rgb(153, 153, 153)",
    );
  });
}
