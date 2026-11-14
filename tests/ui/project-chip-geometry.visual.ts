import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  expectComposerSweep,
  settledComposerFit,
  sweepComposerToolbar,
} from "./shared/composerGeometry";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

import {
  openSelectionScenario,
  dragAnswerPhrase,
} from "./shared/transcriptSelection";

async function openSplitComposerScenario(page: Page) {
  await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  await settleChatProjections(page);
}

async function expectProjectInOptionsMenu(composer: Locator) {
  const page = composer.page();
  const add = composer.getByRole("button", {
    name: "Composer options",
    exact: true,
  });
  await expect(add).toBeEnabled();
  await add.click();
  await expect(
    page
      .getByRole("menu", { name: "Composer options menu", exact: true })
      .getByRole("menuitem", { name: /^Project: / }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(add).toBeFocused();
}

async function expectEffortReachable(composer: Locator, level: string) {
  const effort = composer.getByRole("button", {
    name: `Thinking level: ${level}`,
    exact: true,
  });
  await expect(
    composer.locator(".composer__effort-picker[data-overflow-control]"),
  ).toHaveCount(1);
  if (!(await settledComposerFit(composer)).includes("effort-menu"))
    return expect(effort).toBeVisible();
  const add = composer.getByRole("button", {
    name: "Composer options",
    exact: true,
  });
  await add.click();
  await expect(
    composer
      .page()
      .getByRole("menu", { name: "Composer options menu", exact: true })
      .getByRole("menuitem", { name: "Thinking level", exact: true }),
  ).toBeVisible();
  await composer.page().keyboard.press("Escape");
  await expect(add).toBeFocused();
}

async function expectResponsiveToolbar(composer: Locator) {
  const fit = await settledComposerFit(composer);
  const project = composer.getByRole("button", { name: /^Project: / });
  // The chip leaves the toolbar for the composer options menu only once every
  // lower-priority control has compacted and the row still cannot fit it.
  if (fit.includes("project-menu")) {
    await expectProjectInOptionsMenu(composer);
    return expectToolbarGeometry(composer);
  }
  await expect(project).toBeVisible();
  const name = await project.getAttribute("aria-label");
  await expect(project).toHaveAttribute("title", name!);
  const box = await project.boundingBox();
  expect(box!.width).toBeGreaterThanOrEqual(24);
  expect(box!.height).toBeGreaterThanOrEqual(24);
  // The chip shows its whole name or only its folder icon.
  if (
    !(await project.locator(".searchable-picker__trigger-label").isVisible())
  ) {
    await expect(project.locator(".searchable-picker__chevron")).toBeHidden();
    await expect(project.locator(".project-scope__icon")).toBeVisible();
    expect(box!.width).toBeLessThanOrEqual(28);
  }
  const harness = composer.getByRole("button", {
    name: "Harness",
    exact: true,
  });
  const model = composer.getByLabel("Model", { exact: true });
  for (const trigger of [harness, model]) {
    await expect(trigger).toBeVisible();
    expect((await trigger.boundingBox())!.width).toBeGreaterThanOrEqual(24);
  }
  const harnessLabel = harness.locator(".searchable-picker__trigger-label");
  // The brand mark always stays whole inside the harness trigger.
  const harnessBox = (await harness.boundingBox())!;
  const markBox = (await harness.locator("[data-harness-icon]").boundingBox())!;
  expect(markBox.width).toBeGreaterThanOrEqual(12);
  expect(markBox.x).toBeGreaterThanOrEqual(harnessBox.x - 0.5);
  expect(markBox.x + markBox.width).toBeLessThanOrEqual(
    harnessBox.x + harnessBox.width + 0.5,
  );
  if (await harnessLabel.isVisible()) {
    expect(
      await harnessLabel.evaluate(
        (node) => node.scrollWidth <= node.clientWidth,
      ),
    ).toBe(true);
  }
  if (!(await model.locator(".composer__model-label").isVisible()))
    await expect(model.locator(".composer__model-icon")).toBeVisible();
  await expectToolbarGeometry(composer);
}

async function expectToolbarGeometry(composer: Locator) {
  const geometry = await composer.evaluate((element) => {
    const toolbar = element.querySelector(".composer__toolbar")!;
    const controls = element.querySelector(".composer__controls")!;
    const row = toolbar.getBoundingClientRect();
    const targets = [...controls.children]
      .map((child) => child.getBoundingClientRect())
      .filter((target) => target.width > 0 && target.height > 0);
    const buttons = [...toolbar.querySelectorAll("button")].filter(
      (button) =>
        button.getBoundingClientRect().width > 0 &&
        button.getBoundingClientRect().height > 0,
    );
    const bounds = buttons.map((button) => button.getBoundingClientRect());
    const controlsRow = controls.getBoundingClientRect();
    const mic = toolbar
      .querySelector(".composer__mic")!
      .getBoundingClientRect();
    const send = toolbar.querySelector(".send-button")!.getBoundingClientRect();
    // Only a row that cannot fit even as icons wraps, with the dictation and
    // send actions on a row of their own.
    const secondRow = mic.top >= controlsRow.bottom - 1;
    return {
      overflow: toolbar.scrollWidth > toolbar.clientWidth,
      disjoint: bounds.every((button, index) =>
        bounds
          .slice(index + 1)
          .every(
            (other) =>
              button.right <= other.left ||
              other.right <= button.left ||
              button.bottom <= other.top ||
              other.bottom <= button.top,
          ),
      ),
      hitTestable: buttons.every((button, index) => {
        const target = bounds[index];
        // Chromium hit-tests the rounded shape, so probe each corner just
        // inside its radius instead of the bounding-box corner.
        const radius = getComputedStyle(button).borderTopLeftRadius;
        const half = Math.min(target.width, target.height) / 2;
        const corner = Math.min(
          half,
          radius.endsWith("%")
            ? (Number.parseFloat(radius) / 100) * half * 2
            : Number.parseFloat(radius) || 0,
        );
        const inset = corner * (1 - Math.SQRT1_2) + 2;
        return [
          [target.left + target.width / 2, target.top + target.height / 2],
          [target.left + inset, target.top + inset],
          [target.right - inset, target.bottom - inset],
        ].every(([horizontal, vertical]) =>
          button.contains(document.elementFromPoint(horizontal, vertical)),
        );
      }),
      contained: [...targets, ...bounds].every(
        (target) =>
          target.left >= row.left &&
          target.right <= row.right &&
          target.top >= row.top &&
          target.bottom <= row.bottom,
      ),
      // Mic (28px tall) and the 24px compact send share a centre line.
      actionsAligned:
        Math.abs(mic.top + mic.height / 2 - send.top - send.height / 2) <= 1,
      identityRow:
        secondRow ||
        targets.every(
          (target) =>
            Math.abs(
              target.top +
                target.height / 2 -
                controlsRow.top -
                controlsRow.height / 2,
            ) <= 1,
        ),
      responsiveRows: secondRow
        ? mic.top >= controlsRow.bottom
        : Math.abs(
            mic.top + mic.height / 2 - controlsRow.top - controlsRow.height / 2,
          ) <= 1,
    };
  });
  expect(geometry).toEqual({
    overflow: false,
    disjoint: true,
    hitTestable: true,
    contained: true,
    actionsAligned: true,
    identityRow: true,
    responsiveRows: true,
  });
}

for (const width of [1888, 720, 480]) {
  for (const scenario of ["agent-single", "agent-dual"] as const) {
    test(`Project chip keeps toolbar controls disjoint at ${width} in ${scenario}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, scenario, "agent");
      await settleChatProjections(page);
      for (const composer of await page.locator("form.composer").all()) {
        await expectResponsiveToolbar(composer);
      }
    });
  }
}

for (const contentWidth of [
  128, 184, 252, 259, 259.5, 260, 264, 280, 281, 320, 321, 400, 401,
]) {
  test(`Project toolbar remains hit-testable at ${contentWidth}px content width`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    const composer = page.locator("form.composer").first();
    await composer.evaluate((element, width) => {
      const computed = getComputedStyle(element);
      element.style.width = `${width + Number.parseFloat(computed.borderLeftWidth) + Number.parseFloat(computed.borderRightWidth)}px`;
    }, contentWidth);
    await expectResponsiveToolbar(composer);
    await page.evaluate(async () => {
      const fixture =
        await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
      fixture.installVisualComposerEffortFixture({
        levels: ["low", "medium", "extraordinarily high"],
        currentValue: "extraordinarily high",
      });
    });
    await expectEffortReachable(composer, "Extraordinarily high");
    await expectResponsiveToolbar(composer);
  });
}

for (const paneWidth of [560, 414, 400]) {
  test(`Project chip preserves toolbar labels in a ${paneWidth}px pane`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.addStyleTag({
      content: `.agent-chat-panes { grid-template-columns: ${paneWidth}px; }`,
    });
    const composer = page.locator("form.composer").first();
    await expectResponsiveToolbar(composer);
  });
}

test("composer controls compact by their own container width, not the window", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  // The Goal fixture offers composer options, so the Add button is present.
  await openScenario(page, "agent-composer-goal", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer").first();
  const contentWidths = [580, 521, 520, 401, 400, 321, 320, 281, 280, 180];
  const borders = await composer.evaluate((element) => {
    const computed = getComputedStyle(element);
    return (
      Number.parseFloat(computed.borderLeftWidth) +
      Number.parseFloat(computed.borderRightWidth)
    );
  });
  const widths = contentWidths.map((width) => width + borders);
  // The window never changes, so whatever compacts follows the composer's
  // own width, and only as far as its measured row needs.
  expectComposerSweep(await sweepComposerToolbar(composer, widths));

  const access = composer.getByRole("button", { name: "Access options" });
  const harnessLabel = composer
    .getByRole("button", { name: "Harness", exact: true })
    .locator(".searchable-picker__trigger-label");
  const model = composer.getByRole("button", { name: "Model", exact: true });
  const modelLabel = model.locator(".composer__model-label");
  const modelIcon = model.locator(".composer__model-icon");
  for (const [index, width] of widths.entries()) {
    await composer.evaluate(
      (element, width) => (element.style.width = `${width}px`),
      width,
    );
    const at = `at ${contentWidths[index]}px`;
    expect(await composer.evaluate((element) => element.clientWidth), at).toBe(
      contentWidths[index],
    );
    await settledComposerFit(composer);
    const plus = (await composer
      .getByRole("button", { name: "Composer options", exact: true })
      .boundingBox())!;
    expect(plus.width, at).toBe(24);
    expect(plus.height, at).toBe(28);
    // The access control stays a target with or without its text.
    await expect(access, at).toBeVisible();
    expect((await access.boundingBox())!.width, at).toBeGreaterThanOrEqual(24);
    // The harness name shows whole or not at all, leaving its mark.
    if (await harnessLabel.isVisible())
      expect(
        await harnessLabel.evaluate(
          (label) => label.scrollWidth <= label.clientWidth,
        ),
        at,
      ).toBe(true);
    await expect(composer.locator(".composer__shortcut"), at).toBeHidden();
    if (!(await modelLabel.isVisible())) {
      // The model name gives way to its icon, never to a bare chevron.
      await expect(modelIcon, at).toBeVisible();
      continue;
    }
    await expect(modelIcon, at).toBeHidden();
    // The label truncates inside its own box, before the chevron, without
    // clipping the trigger's leading edge.
    const geometry = await model.evaluate((trigger) => {
      const label = trigger.querySelector(".composer__model-label")!;
      const chevron = trigger.querySelector("svg:last-child")!;
      const box = trigger.getBoundingClientRect();
      const text = label.getBoundingClientRect();
      const glyph = chevron.getBoundingClientRect();
      return {
        labelStart: text.left - box.left,
        beforeChevron: text.right <= glyph.left + 0.5,
        chevron: [glyph.width, glyph.height],
        chevronInside: glyph.left >= box.left && glyph.right <= box.right + 0.5,
        ellipsis: getComputedStyle(label).textOverflow,
      };
    });
    expect(geometry.labelStart, at).toBeGreaterThanOrEqual(0);
    expect(geometry.beforeChevron, at).toBe(true);
    expect(geometry.chevron, at).toEqual([16, 16]);
    expect(geometry.chevronInside, at).toBe(true);
    expect(geometry.ellipsis, at).toBe("ellipsis");
  }
});

test.describe("Project chip 200% zoom reflow proxy", () => {
  test.use({ viewport: { width: 480, height: 360 }, deviceScaleFactor: 2 });
  test("keeps the narrow toolbar and editable textarea contained", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    expect(await page.evaluate(() => window.devicePixelRatio)).toBe(2);
    const composer = page.locator("form.composer").first();
    await expectResponsiveToolbar(composer);
    const textarea = composer.getByRole("combobox", { name: "Message agent" });
    await textarea.fill("Draft remains editable at 200% reflow");
    await expect(textarea).toHaveValue("Draft remains editable at 200% reflow");
    const field = (await textarea.boundingBox())!;
    const form = (await composer.boundingBox())!;
    const toolbar = (await composer
      .locator(".composer__toolbar")
      .boundingBox())!;
    // The 360 px viewport uses the compact 40–56 px textarea range.
    expect(field.height).toBeGreaterThanOrEqual(40);
    expect(field.height).toBeLessThanOrEqual(56);
    expect(field.y).toBeGreaterThanOrEqual(form.y);
    expect(field.y + field.height).toBeLessThanOrEqual(toolbar.y);
    expect(toolbar.y + toolbar.height).toBeLessThanOrEqual(
      form.y + form.height,
    );
    expect(form.y + form.height).toBeLessThanOrEqual(360);
  });
});

// These geometry contracts used to inspect CSS selectors in jsdom. Exercise
// the rendered controls so stylesheet ownership can change independently.
test("keeps the split chip nonshrinking after the later composer styles load", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openSplitComposerScenario(page);
  await expect(
    page.locator("form.composer").nth(1).locator(".project-scope"),
  ).toHaveCount(0);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
    fixture.installVisualComposerEffortFixture({
      modelName: "A long advertised model name",
      currentValue: "high",
    });
  });
  for (const composer of [page.locator("form.composer").first()]) {
    await composer.evaluate((element) => (element.style.width = "440px"));
    // At this width every other label is an icon and the chip is the last
    // toolbar control before it moves into the options menu.
    await settledComposerFit(composer);
    const chip = composer.locator(".project-scope");
    const trigger = chip.getByRole("button");
    await expect(chip).toHaveCSS("width", "28px");
    await expect(chip).toHaveCSS("flex-shrink", "0");
    await expect(trigger).toHaveCSS("min-width", "28px");
    await expect(trigger).toHaveCSS("min-height", "28px");
    await expect(
      trigger.locator(".searchable-picker__trigger-label"),
    ).toBeHidden();
    await expect(trigger.locator(".searchable-picker__chevron")).toBeHidden();
  }
});

test("reserves clickable icon targets while allowing the harness to shrink", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer").first();
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
    fixture.installVisualComposerEffortFixture({
      levels: ["low", "medium", "high"],
      currentValue: "high",
    });
  });
  const project = composer.getByRole("button", { name: /^Project: / });
  const model = composer.getByLabel("Model", { exact: true });
  const effort = composer.getByRole("button", { name: "Thinking level: High" });
  await expect(composer.locator(".project-scope")).toHaveCSS(
    "min-width",
    "28px",
  );
  for (const trigger of [project, model, effort]) {
    await expect(trigger).toHaveCSS("min-width", "28px");
    expect((await trigger.boundingBox())!.width).toBeGreaterThanOrEqual(28);
  }
  await expect(project).toHaveCSS("min-height", "28px");
  await expect(effort).toHaveCSS("overflow", "hidden");
  await composer.evaluate((element) => {
    element.style.width = "320px";
  });
  await expectResponsiveToolbar(composer);
});

test("uses the same nonshrinking icon-only chip in narrow and split composers", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  for (const scenario of ["agent-single", "agent-dual"] as const) {
    if (scenario === "agent-dual") await openSplitComposerScenario(page);
    else await openScenario(page, scenario, "agent");
    await settleChatProjections(page);
    if (scenario === "agent-dual")
      await expect(
        page.locator("form.composer").nth(1).locator(".project-scope"),
      ).toHaveCount(0);
    await page.evaluate(async () => {
      const fixture =
        await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
      fixture.installVisualComposerEffortFixture({
        modelName: "A long advertised model name",
        currentValue: "high",
      });
    });
    for (const composer of [page.locator("form.composer").first()]) {
      await composer.evaluate((element) => (element.style.width = "440px"));
      await settledComposerFit(composer);
      if (scenario === "agent-single") {
        await composer.evaluate((element) => {
          element.style.width = "440px";
        });
      }
      const chip = composer.locator(".project-scope");
      const trigger = chip.getByRole("button");
      await expect(chip).toHaveCSS("width", "28px");
      await expect(chip).toHaveCSS("flex-shrink", "0");
      await expect(trigger).toHaveCSS("justify-content", "center");
      await expect(trigger).toHaveCSS("padding", "0px");
      await expect(
        trigger.locator(".searchable-picker__trigger-label"),
      ).toBeHidden();
      await expect(trigger.locator(".searchable-picker__chevron")).toBeHidden();
      await expect(trigger.locator(".project-scope__icon")).toBeVisible();
      await expectResponsiveToolbar(composer);
    }
  }
});
