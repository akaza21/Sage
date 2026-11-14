import { expect, test, type Locator } from "@playwright/test";

import { releasePickerHarnessReadiness } from "./agentVisualHelpers";
import {
  waitForAnimations,
  openScenario,
} from "./shared/visualBrowserPrimitives";

test("gives harness and model triggers the same control treatment", async ({
  page,
}) => {
  await openScenario(page, "agent-picker-harness-model", "agent");
  const composer = page.locator("form.composer").first();
  const harness = composer.getByRole("button", {
    name: "Harness",
    exact: true,
  });
  const model = composer.getByRole("button", { name: "Model", exact: true });
  const treatment = (control: typeof harness) =>
    control.evaluate((element) => {
      const style = getComputedStyle(element);
      return [
        style.height,
        style.fontSize,
        style.fontWeight,
        style.borderWidth,
        style.borderRadius,
        style.backgroundColor,
      ];
    });
  expect(await treatment(harness)).toEqual(await treatment(model));
  await harness.hover();
  const harnessHover = await harness.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  );
  await expect(composer.locator(".composer__harness")).toHaveCSS(
    "background-color",
    "rgba(0, 0, 0, 0)",
  );
  await model.hover();
  await expect(model).toHaveCSS("background-color", harnessHover);
  expect(harnessHover).not.toBe("rgba(0, 0, 0, 0)");
  for (const [icon, size] of [
    [harness.locator("[data-harness-icon]"), "16px"],
    [model.locator("svg").last(), "16px"],
  ] as const) {
    await expect(icon).toBeVisible();
    await expect(icon).toHaveCSS("width", size);
    await expect(icon).toHaveCSS("height", size);
  }
});

test("gives single-line harness rows bounded scrolling and visible keyboard focus", async ({
  page,
}) => {
  await openScenario(page, "agent-picker-harness-model", "agent");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    useWorkbenchStore.setState((state) => ({
      harnesses: Array.from({ length: 24 }, (_, index) => ({
        ...state.harnesses[0],
        id: `harness-${index}`,
        name: `Harness ${index + 1}`,
      })),
    }));
  });
  const harness = page.getByRole("button", { name: "Harness", exact: true });
  await harness.focus();
  await page.keyboard.press("Enter");
  const list = page.getByRole("listbox", { name: "Harness", exact: true });
  const row = list.getByRole("option").first();
  const rowBox = (await row.boundingBox())!;
  expect(rowBox.height).toBeGreaterThanOrEqual(32);
  expect(rowBox.height).toBeLessThanOrEqual(36);
  const textHeight = await row
    .locator(".searchable-picker__option-copy > span")
    .evaluate((node) => {
      const range = document.createRange();
      range.selectNodeContents(node);
      return range.getBoundingClientRect().height;
    });
  expect(textHeight).toBeLessThanOrEqual(rowBox.height);
  await expect(row).toBeFocused();
  expect(
    await row.evaluate((element) => element.matches(":focus-visible")),
  ).toBe(true);
  // Rows ease into their focus fill; read the fill it settles on.
  await waitForAnimations(row, { subtree: false });
  const keyboard = await row.evaluate((element) => ({
    fill: getComputedStyle(element).backgroundColor,
    shadow: getComputedStyle(element).boxShadow,
  }));
  expect(keyboard.fill).toBe("rgb(68, 68, 68)");
  expect(keyboard.shadow).not.toBe("none");
  expect(
    await list.evaluate((node) => node.scrollHeight > node.clientHeight),
  ).toBe(true);
  await page.keyboard.press("End");
  const last = list.getByRole("option").last();
  await expect(last).toBeFocused();
  const listBox = (await list.boundingBox())!;
  const lastBox = (await last.boundingBox())!;
  expect(lastBox.y).toBeGreaterThanOrEqual(listBox.y);
  expect(lastBox.y + lastBox.height).toBeLessThanOrEqual(
    listBox.y + listBox.height,
  );
  await page.keyboard.press("Escape");
  await expect(harness).toBeFocused();
  await expect(list).toHaveCount(0);
});

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
    const menu = page.locator(".composer__model-menu");
    const paint = await menu.evaluate((popover) => {
      const system = document.createElement("span");
      system.style.cssText = "color: Highlight; background-color: Canvas";
      document.body.append(system);
      const systemStyle = getComputedStyle(system);
      const title = popover.querySelector(".composer-model-picker__title")!;
      const titleStyle = getComputedStyle(title);
      const titleBox = title.getBoundingClientRect();
      const options = [...popover.querySelectorAll('[role="option"]')];
      const focused = options.find(
        (option) => option === document.activeElement,
      )!;
      const result = {
        canvas: systemStyle.backgroundColor,
        highlight: systemStyle.color,
        popover: getComputedStyle(popover).backgroundColor,
        focusedFill: getComputedStyle(focused).backgroundColor,
        // A whole-pixel title whose line box is 16px inside its padding.
        titleLine:
          title.clientHeight -
          Number.parseFloat(titleStyle.paddingTop) -
          Number.parseFloat(titleStyle.paddingBottom),
        titleWhole:
          Number.isInteger(titleBox.top) && Number.isInteger(titleBox.height),
        optionHeights: [
          ...new Set(
            options
              .slice(0, 8)
              .map((option) => option.getBoundingClientRect().height),
          ),
        ],
      };
      system.remove();
      return result;
    });
    expect(paint.titleLine).toBe(16);
    expect(paint.titleWhole).toBe(true);
    // Compact options: one 28px row each.
    expect(paint.optionHeights).toEqual([28]);
    if (forcedColors === "active") {
      expect(paint.popover).toBe(paint.canvas);
      expect(paint.focusedFill).toBe(paint.highlight);
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
  const composerBox = await page.locator(".chat-dock").first().boundingBox();
  expect(composerBox).not.toBeNull();
  const columnBox = await page
    .locator(".message-list__content")
    .first()
    .boundingBox();
  expect(columnBox).not.toBeNull();
  expect(composerBox!.width).toBeCloseTo(columnBox!.width, 1);
  expect(composerBox!.x).toBeCloseTo(columnBox!.x, 1);
  // A single rounded card with a hairline edge and a round, white send.
  const footprint = await page
    .locator(".chat-dock")
    .first()
    .evaluate((card) => {
      const style = getComputedStyle(card);
      const send = card.querySelector(".send-button")!;
      const sendStyle = getComputedStyle(send);
      const sendBox = send.getBoundingClientRect();
      return {
        borders: [
          style.borderTopWidth,
          style.borderRightWidth,
          style.borderBottomWidth,
          style.borderLeftWidth,
        ],
        radius: Number.parseFloat(style.borderTopLeftRadius),
        send: [sendBox.width, sendBox.height],
        sendRadius: Number.parseFloat(sendStyle.borderTopLeftRadius),
        sendFill: sendStyle.backgroundColor.match(/[\d.]+/g)!.map(Number),
      };
    });
  expect(footprint.borders).toEqual(["1px", "1px", "1px", "1px"]);
  expect(footprint.radius).toBeGreaterThanOrEqual(12);
  expect(footprint.send).toEqual([28, 28]);
  expect(footprint.sendRadius).toBeGreaterThanOrEqual(14);
  for (const channel of footprint.sendFill.slice(0, 3))
    expect(channel).toBeGreaterThanOrEqual(230);

  const harness = composer.getByRole("button", { name: "Harness" });
  const triggerBox = await harness.boundingBox();
  expect(triggerBox).not.toBeNull();
  const wrapperBox = (await composer
    .locator(".composer__harness")
    .boundingBox())!;
  const promptBox = (await composer
    .getByRole("combobox", { name: "Message agent", exact: true })
    .boundingBox())!;
  expect(triggerBox!.y).toBeGreaterThanOrEqual(promptBox.y + promptBox.height);
  expect(triggerBox!.y).toBeGreaterThanOrEqual(wrapperBox.y);
  expect(triggerBox!.y + triggerBox!.height).toBeLessThanOrEqual(
    wrapperBox.y + wrapperBox.height,
  );
  await harness.click({ trial: true });
  // A plain mark-and-name control sized to its label, never a fixed pill.
  expect(triggerBox!.width).toBeLessThanOrEqual(160);
  expect(
    await harness
      .locator(".searchable-picker__trigger-label")
      .evaluate((label) => label.scrollWidth <= label.clientWidth),
  ).toBe(true);
  // The trigger takes its name's width, so the chevron follows the name.
  const [nameBox, chevronBox] = await Promise.all([
    harness.locator(".searchable-picker__trigger-label").boundingBox(),
    harness.locator(".searchable-picker__chevron").boundingBox(),
  ]);
  expect(chevronBox!.x - (nameBox!.x + nameBox!.width)).toBeLessThanOrEqual(
    4.5,
  );

  const popoverPaint = (popover: Locator) =>
    popover.evaluate((element) => {
      const style = getComputedStyle(element);
      return [
        style.borderTopWidth,
        style.borderTopColor,
        style.backgroundColor,
        style.color,
        style.boxShadow,
        style.borderTopLeftRadius,
      ];
    });
  await harness.click();
  const listbox = page.getByRole("listbox", { name: "Harness" });
  const card = listbox.locator(
    "xpath=ancestor::div[contains(@class, 'menu-popover')][1]",
  );
  await waitForAnimations(card);
  const harnessPaint = await popoverPaint(card);
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
  const modelPopover = modelCard.locator(
    "xpath=ancestor::div[contains(@class, 'menu-popover')][1]",
  );
  await waitForAnimations(modelPopover);
  // Harness and model menus share one popover treatment.
  expect(await popoverPaint(modelPopover)).toEqual(harnessPaint);
  const modelBox = await modelPopover.boundingBox();
  expect(modelBox).not.toBeNull();
  expect(modelBox!.width).toBeGreaterThanOrEqual(228);
  expect(modelBox!.width).toBeLessThanOrEqual(236);
  expect(modelBox!.height).toBeLessThanOrEqual(312);
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
  const insets = await goal.evaluate((element) => {
    const form = element.closest("form")!;
    const style = getComputedStyle(form);
    return (
      2 *
      (Number.parseFloat(style.getPropertyValue("--space-3")) +
        Number.parseFloat(style.borderLeftWidth))
    );
  });
  expect(goalBox!.width).toBeCloseTo(composerBox!.width - insets, 1);
  expect(goalBox!.x - composerBox!.x).toBeGreaterThanOrEqual(8);
  expect(goalBox!.x - composerBox!.x).toBeLessThanOrEqual(16);
});

for (const forcedColors of ["none", "active"] as const) {
  test(`token meter sits in the toolbar with a keyboard focus ring and system edges (${forcedColors})`, async ({
    page,
  }) => {
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "agent-composer-goal", "agent");
    const composer = page.locator("form.composer").first();
    const meter = composer.getByRole("progressbar", {
      name: "6,000 used / 272,000 total tokens",
    });
    await expect(meter).toBeVisible();
    // The meter is laid out in the toolbar row between the controls and
    // the send action, not floated over the prompt.
    const placement = await meter.evaluate((element) => {
      const toolbar = element.closest(".composer__toolbar")!;
      const row = toolbar.getBoundingClientRect();
      const box = element.getBoundingClientRect();
      const controls = toolbar
        .querySelector(".composer__controls")!
        .getBoundingClientRect();
      const send = toolbar
        .querySelector(".send-button")!
        .getBoundingClientRect();
      const prompt = element
        .closest("form")!
        .querySelector("textarea")!
        .getBoundingClientRect();
      return {
        inRow: box.top >= row.top && box.bottom <= row.bottom,
        afterControls: box.left >= controls.right,
        beforeSend: box.right <= send.left,
        belowPrompt: box.top >= prompt.bottom,
      };
    });
    expect(placement).toEqual({
      inRow: true,
      afterControls: true,
      beforeSend: true,
      belowPrompt: true,
    });
    await composer.getByRole("combobox", { name: "Message agent" }).focus();
    await page.keyboard.press("Tab");
    while (
      !(await meter.evaluate((element) => element === document.activeElement))
    )
      await page.keyboard.press("Tab");
    const ring = await meter.evaluate((element) => {
      const style = getComputedStyle(element);
      return {
        focusVisible: element.matches(":focus-visible"),
        style: style.outlineStyle,
        width: Number.parseFloat(style.outlineWidth),
      };
    });
    expect(ring.focusVisible).toBe(true);
    expect(ring.style).not.toBe("none");
    expect(ring.width).toBeGreaterThan(0);
    if (forcedColors === "active") {
      const edges = await meter.evaluate((element) => {
        const system = document.createElement("span");
        system.style.cssText = "color: CanvasText; background-color: Highlight";
        document.body.append(system);
        const systemStyle = getComputedStyle(system);
        const fill = getComputedStyle(element, "::before");
        const send = getComputedStyle(
          element.closest("form")!.querySelector(".send-button")!,
        );
        const result = {
          canvasText: systemStyle.color,
          highlight: systemStyle.backgroundColor,
          fillBorder: [fill.borderTopStyle, fill.borderTopColor],
          fillAdjust: fill.forcedColorAdjust,
          fillImage: fill.backgroundImage,
          sendBorder: [
            send.borderTopWidth,
            send.borderTopStyle,
            send.borderTopColor,
          ],
        };
        system.remove();
        return result;
      });
      expect(edges.fillBorder).toEqual(["solid", edges.canvasText]);
      // The used share keeps a system Highlight fill instead of being
      // flattened away by forced colors.
      expect(edges.fillAdjust).toBe("none");
      expect(edges.fillImage).toContain("conic-gradient");
      expect(edges.fillImage).toContain(edges.highlight);
      expect(edges.sendBorder).toEqual(["1px", "solid", edges.canvasText]);
    }
  });
}

test("a long harness list scrolls inside its menu and shows keyboard focus", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 600 });
  await openScenario(page, "agent-picker-harness-model", "agent");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    const template = state.harnesses[0];
    useWorkbenchStore.setState({
      harnesses: [
        ...state.harnesses,
        ...Array.from({ length: 30 }, (_, index) => ({
          ...template,
          id: `geometry-harness-${index}`,
          name: `Geometry harness ${index}`,
        })),
      ],
    });
  });
  const harness = page
    .locator("form.composer")
    .first()
    .getByRole("button", { name: "Harness" });
  await harness.focus();
  await page.keyboard.press("Enter");
  const listbox = page.getByRole("listbox", { name: "Harness" });
  const popover = listbox.locator(
    "xpath=ancestor::div[contains(@class, 'menu-popover')][1]",
  );
  await expect(listbox.getByRole("option").first()).toBeFocused();
  const bounds = await popover.boundingBox();
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(600);
  expect(
    await listbox.evaluate(
      (element) => element.scrollHeight > element.clientHeight,
    ),
  ).toBe(true);
  const rows = await listbox
    .getByRole("option")
    .evaluateAll((options) =>
      options
        .slice(0, 6)
        .map((option) => option.getBoundingClientRect().height),
    );
  for (const height of rows) expect(height).toBeGreaterThanOrEqual(32);
  // Rows ease into their focus fill; read the fill each settles on.
  const fill = async (option: Locator) => {
    await waitForAnimations(option, { subtree: false });
    return option.evaluate(
      (element) => getComputedStyle(element).backgroundColor,
    );
  };
  const resting = await fill(listbox.getByRole("option").nth(1));
  await page.keyboard.press("End");
  // End lands on the last selectable row; unavailable harnesses follow it.
  const last = listbox.locator('[role="option"]:not([disabled])').last();
  await expect(last).toBeFocused();
  expect(
    await last.evaluate((element) => element.matches(":focus-visible")),
  ).toBe(true);
  expect(await fill(last)).not.toBe(resting);
  const lastBox = (await last.boundingBox())!;
  const listBox = (await listbox.boundingBox())!;
  expect(lastBox.y).toBeGreaterThanOrEqual(listBox.y);
  expect(lastBox.y + lastBox.height).toBeLessThanOrEqual(
    listBox.y + listBox.height + 0.5,
  );
});
