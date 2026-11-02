import { expect, test } from "@playwright/test";

import { releasePickerHarnessReadiness } from "./agentVisualHelpers";
import { openScenario } from "./shared/visualBrowserPrimitives";

for (const forcedColors of ["none", "active"] as const) {
  test(`asserts no harness pointer or blur focus ring and preserves keyboard focus (${forcedColors})`, async ({
    page,
  }) => {
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await openScenario(page, "agent-picker-harness-model", "agent");
    await page
      .locator(".agent-task-header")
      .first()
      .click({ position: { x: 8, y: 8 } });
    const composer = page.locator("form.composer").first();
    const harness = composer.getByRole("button", { name: "Harness" });
    const focusState = () =>
      harness.evaluate((element) => {
        const active = document.activeElement;
        const style = getComputedStyle(element);
        const composerStyle = getComputedStyle(element.closest("form")!);
        return {
          activeElement: active?.tagName,
          activeLabel: active?.getAttribute("aria-label"),
          focusVisible: element.matches(":focus-visible"),
          outline: style.outline,
          outlineStyle: style.outlineStyle,
          outlineWidth: style.outlineWidth,
          // Chromium reports the initial `medium` (3px) width even when
          // outline-style is none; CSS UI 4 computes that width as zero.
          paintedOutlineWidth:
            style.outlineStyle === "none" ? "0px" : style.outlineWidth,
          shadow: style.boxShadow,
          composerOutline: composerStyle.outline,
          composerBorder: composerStyle.borderColor,
          composerShadow: composerStyle.boxShadow,
        };
      });
    const resting = await focusState();
    await harness.click();
    await page
      .getByRole("option", { name: "Codex — test only", exact: true })
      .click();
    await expect(harness).toBeFocused();
    const pointerSelected = await focusState();
    expect(pointerSelected).toMatchObject({
      focusVisible: false,
      outlineStyle: "none",
      paintedOutlineWidth: "0px",
      shadow: "none",
    });
    await page
      .locator(".agent-task-header")
      .first()
      .click({ position: { x: 8, y: 8 } });
    await expect(harness).not.toBeFocused();
    const blurred = await focusState();
    expect(blurred).toMatchObject({
      focusVisible: false,
      outlineStyle: "none",
      paintedOutlineWidth: "0px",
      shadow: "none",
    });
    await test.info().attach("harness-focus-diagnostics", {
      body: JSON.stringify({ resting, pointerSelected, blurred }, null, 2),
      contentType: "application/json",
    });
    await harness.focus();
    await page.keyboard.press("Enter");
    await expect(
      page.getByRole("option", { name: "Codex — test only", exact: true }),
    ).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(harness).toBeFocused();
    await expect
      .poll(() =>
        harness.evaluate((element) => ({
          focusVisible: element.matches(":focus-visible"),
          outline: getComputedStyle(element).outlineStyle,
        })),
      )
      .toEqual({ focusVisible: true, outline: "solid" });
  });
}

for (const forcedColors of ["none", "active"] as const) {
  test(`model list pointer reopening clears focus-visible and keyboard navigation restores it (${forcedColors})`, async ({
    page,
  }) => {
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await openScenario(page, "agent-picker-harness-model", "agent");
    const composer = page.locator("form.composer").first();
    // The readiness fixture is armed only once a Harness is selected.
    await composer.getByRole("button", { name: "Harness" }).click();
    await page
      .getByRole("option", { name: "Codex — test only", exact: true })
      .click();
    const model = composer.getByRole("button", { name: "Model", exact: true });
    await expect(model).toBeDisabled();
    await releasePickerHarnessReadiness(page);
    await expect(model).toBeEnabled();
    await model.click();
    await page
      .getByRole("option", { name: "Fresh advertised model 2", exact: true })
      .click();
    await page.keyboard.press("Enter");
    const selected = page.getByRole("option", {
      name: "Fresh advertised model 2",
      exact: true,
    });
    await expect(selected).toHaveAttribute("aria-selected", "true");
    await expect(selected).toBeFocused();
    const focusState = () =>
      selected.evaluate((element) => {
        const style = getComputedStyle(element);
        return {
          focusVisible: element.matches(":focus-visible"),
          outlineStyle: style.outlineStyle,
          shadow: style.boxShadow,
        };
      });
    const keyboardFocused = await focusState();
    expect(keyboardFocused.focusVisible).toBe(true);
    if (forcedColors === "active") {
      expect(keyboardFocused).toEqual({
        focusVisible: true,
        outlineStyle: "none",
        shadow: "none",
      });
    } else {
      expect(keyboardFocused.shadow).not.toBe("none");
    }
    await page.keyboard.press("Escape");
    await expect(model).toBeFocused();
    await model.click();
    await expect(selected).toBeFocused();
    expect(await focusState()).toEqual({
      focusVisible: false,
      outlineStyle: "none",
      shadow: "none",
    });
    await page.keyboard.press("Home");
    await page.keyboard.press("ArrowDown");
    await expect(selected).toBeFocused();
    expect(await focusState()).toEqual(keyboardFocused);
  });
}

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
  expect(triggerBox!.width).toBeLessThanOrEqual(132);
  // The trigger takes its name's width, so the chevron follows the name.
  const [nameBox, chevronBox] = await Promise.all([
    harness.locator(".searchable-picker__trigger-label").boundingBox(),
    harness.locator(".searchable-picker__chevron").boundingBox(),
  ]);
  expect(chevronBox!.x - (nameBox!.x + nameBox!.width)).toBeLessThanOrEqual(
    4.5,
  );

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
  expect(rowBox!.height).toBeGreaterThanOrEqual(32);
  expect(rowBox!.height).toBeLessThanOrEqual(36);
  const finalRowBox = await listbox.getByRole("option").last().boundingBox();
  const listBox = await listbox.boundingBox();
  expect(finalRowBox).not.toBeNull();
  expect(listBox).not.toBeNull();
  expect(finalRowBox!.y + finalRowBox!.height).toBeLessThanOrEqual(
    listBox!.y + listBox!.height,
  );

  await row.click();
  await expect(listbox).toHaveCount(0);
  const model = composer.getByRole("button", { name: "Model", exact: true });
  await expect(model).toBeDisabled();
  await releasePickerHarnessReadiness(page);
  await expect(model).toBeEnabled();
  await expect(model).toHaveText("Model");
  await model.click();
  const modelCard = page.getByRole("listbox", {
    name: "Select model",
    exact: true,
  });
  await expect(modelCard.getByRole("option").first()).toBeFocused();
  await expect(modelCard.getByRole("combobox")).toHaveCount(0);
  await expect(modelCard.getByRole("option")).toHaveCount(128);
  await expect(
    modelCard.getByRole("option", {
      name: "Fresh advertised model 1",
      exact: true,
    }),
  ).toBeVisible();
  await expect(modelCard.locator('[aria-selected="true"]')).toHaveCount(0);
  const modelBox = await modelCard
    .locator("xpath=ancestor::div[contains(@class, 'menu-popover')][1]")
    .boundingBox();
  expect(modelBox).not.toBeNull();
  expect(modelBox!.width).toBeGreaterThanOrEqual(228);
  expect(modelBox!.width).toBeLessThanOrEqual(236);
  expect(modelBox!.height).toBeLessThanOrEqual(288);
  await page.keyboard.press("End");
  const lastModel = modelCard.getByRole("option", {
    name: "Fresh advertised model 128",
    exact: true,
  });
  await expect(lastModel).toBeFocused();
  const lastBox = await lastModel.boundingBox();
  const scrollBox = await modelCard.boundingBox();
  expect(lastBox).not.toBeNull();
  expect(scrollBox).not.toBeNull();
  expect(Number.isInteger(scrollBox!.height)).toBe(true);
  expect(lastBox!.y).toBeGreaterThanOrEqual(scrollBox!.y);
  expect(lastBox!.y + lastBox!.height).toBeLessThanOrEqual(
    scrollBox!.y + scrollBox!.height,
  );
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
