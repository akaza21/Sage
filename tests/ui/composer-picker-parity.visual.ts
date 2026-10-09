import { expect, test } from "@playwright/test";

import { releasePickerHarnessReadiness } from "./agentVisualHelpers";
import { openScenario } from "./shared/visualBrowserPrimitives";

test("composer and harness picker match the owner's 96-DPI geometry", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-picker-harness-model", "agent");

  const composer = page.locator("form.composer").first();
  const composerBox = await composer.boundingBox();
  expect(composerBox).not.toBeNull();
  expect(composerBox!.width).toBeGreaterThanOrEqual(656);
  expect(composerBox!.width).toBeLessThanOrEqual(672);

  const harness = composer.getByRole("button", { name: "Harness" });
  const triggerBox = await harness.boundingBox();
  expect(triggerBox).not.toBeNull();
  expect(triggerBox!.width).toBeGreaterThanOrEqual(124);
  expect(triggerBox!.width).toBeLessThanOrEqual(132);

  await harness.click();
  const listbox = page.getByRole("listbox", { name: "Harness" });
  const card = listbox.locator(
    "xpath=ancestor::div[contains(@class, 'menu-popover')][1]",
  );
  const cardBox = await card.boundingBox();
  expect(cardBox).not.toBeNull();
  expect(cardBox!.width).toBeGreaterThanOrEqual(228);
  expect(cardBox!.width).toBeLessThanOrEqual(236);

  const row = listbox.getByRole("option").first();
  await expect(row).toHaveAccessibleName("Codex — test only");
  const rowBox = await row.boundingBox();
  expect(rowBox).not.toBeNull();
  expect(rowBox!.height).toBeGreaterThanOrEqual(48);
  expect(rowBox!.height).toBeLessThanOrEqual(52);

  await row.click();
  await expect(listbox).toHaveCount(0);
  const model = composer.getByRole("button", { name: "Model", exact: true });
  await expect(model).toBeDisabled();
  await releasePickerHarnessReadiness(page);
  await expect(model).toBeEnabled();
  await expect(model).toHaveText("Model");
  await model.click();
  const modelCard = page.getByRole("dialog", { name: "Model picker" });
  await expect(
    modelCard.getByRole("combobox", { name: "Search models" }),
  ).toBeFocused();
  const models = modelCard.getByRole("grid", { name: "Models", exact: true });
  await expect(models.getByRole("row")).toHaveCount(128);
  await expect(
    models.getByRole("gridcell", {
      name: "Fresh advertised model 1 — Codex",
      exact: true,
    }),
  ).toBeVisible();
  await expect(models.locator('[aria-selected="true"]')).toHaveCount(0);
  const modelBox = await modelCard.boundingBox();
  expect(modelBox).not.toBeNull();
  expect(modelBox!.width).toBeGreaterThanOrEqual(316);
  expect(modelBox!.width).toBeLessThanOrEqual(324);
});

test("known context and Goal fit the reference composer without invented native data", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-composer-goal", "agent");
  const composer = page.locator("form.composer").first();
  const goal = composer.getByLabel("Goal details", { exact: true });
  const meter = composer.getByRole("progressbar", {
    name: "6,000 used / 272,000 total tokens",
  });
  await expect(goal).toBeVisible();
  await expect(meter).toBeVisible();
  await expect(meter).toHaveAttribute("aria-valuenow", "6000");
  const composerBox = await composer.boundingBox();
  const goalBox = await goal.boundingBox();
  expect(composerBox).not.toBeNull();
  expect(goalBox).not.toBeNull();
  expect(goalBox!.height).toBe(32);
  expect(goalBox!.width).toBeGreaterThanOrEqual(634);
  expect(goalBox!.width).toBeLessThanOrEqual(642);
  expect(goalBox!.x - composerBox!.x).toBeGreaterThanOrEqual(8);
  expect(goalBox!.x - composerBox!.x).toBeLessThanOrEqual(16);
});
