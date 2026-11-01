import { expect, test, type Locator } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

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

async function expectResponsiveToolbar(
  composer: Locator,
  compact: boolean,
  // A long effort label may take the harness name's room; its mark stays.
  harnessNamed = true,
) {
  const project = composer.getByRole("button", { name: /^Project: / });
  // Wave 5: at 280px and below the Project control folds into the composer
  // options menu so the toolbar keeps one row (Codex ref 37).
  if (await composer.evaluate((element) => element.clientWidth <= 280)) {
    await expect(project).toHaveCount(0);
    await expectProjectInOptionsMenu(composer);
    return expectToolbarGeometry(composer);
  }
  await expect(project).toBeVisible();
  const name = await project.getAttribute("aria-label");
  await expect(project).toHaveAttribute("title", name!);
  const box = await project.boundingBox();
  expect(box!.width).toBeGreaterThanOrEqual(24);
  expect(box!.height).toBeGreaterThanOrEqual(24);
  if (compact) {
    expect(box!.width).toBe(
      await composer.evaluate((element) =>
        element.clientWidth < 280 ? 24 : 28,
      ),
    );
    expect(box!.height).toBe(28);
  }
  const controlsCompact = await composer.evaluate(
    (element) => element.clientWidth <= 280,
  );
  const harness = composer.getByRole("button", {
    name: "Harness",
    exact: true,
  });
  const model = composer.getByRole("button", { name: "Model", exact: true });
  for (const [trigger, minimum] of [
    [harness, 24],
    [model, 24],
  ] as const) {
    await expect(trigger).toBeVisible();
    expect((await trigger.boundingBox())!.width).toBeGreaterThanOrEqual(
      minimum,
    );
  }
  const harnessLabel = harness.locator(".searchable-picker__trigger-label");
  const modelLabel = model.locator(".composer__model-label");
  // The harness keeps its name beside its mark until the whole toolbar
  // compacts to icons.
  if (controlsCompact) await expect(harnessLabel).toBeHidden();
  else if (harnessNamed) await expect(harnessLabel).toBeVisible();
  if (controlsCompact) {
    await expect(modelLabel).toBeHidden();
    await expect(model.locator(".composer__model-icon")).toBeVisible();
  } else if (await composer.evaluate((element) => element.clientWidth > 320)) {
    await expect(modelLabel).toBeVisible();
  } else if (!(await modelLabel.isVisible())) {
    // Measured compaction may drop the label before the 280px query.
    await expect(model.locator(".composer__model-icon")).toBeVisible();
  }
  if (compact) {
    await expect(
      project.locator(".searchable-picker__trigger-label"),
    ).toBeHidden();
    await expect(project.locator(".searchable-picker__chevron")).toBeHidden();
    await expect(project.locator(".project-scope__icon")).toBeVisible();
    await expect(project.locator("..").locator("..")).toHaveCSS(
      "flex-shrink",
      "0",
    );
  }
  await expectToolbarGeometry(composer);
}

async function expectToolbarGeometry(composer: Locator) {
  const geometry = await composer.evaluate((element) => {
    const toolbar = element.querySelector(".composer__toolbar")!;
    const controls = element.querySelector(".composer__controls")!;
    const row = toolbar.getBoundingClientRect();
    const targets = [...controls.children].map((child) =>
      child.getBoundingClientRect(),
    );
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
    const computed = getComputedStyle(element);
    const contentWidth =
      element.getBoundingClientRect().width -
      Number.parseFloat(computed.borderLeftWidth) -
      Number.parseFloat(computed.borderRightWidth) -
      Number.parseFloat(computed.paddingLeft) -
      Number.parseFloat(computed.paddingRight);
    // Wave 5 keeps one toolbar row down to the 182px container rule.
    const secondRow = contentWidth < 182;
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
        const compact = await composer.evaluate(
          (element) =>
            element.clientWidth <= 400 ||
            Boolean(element.closest(".agent-chat-panes--split")),
        );
        await expectResponsiveToolbar(composer, compact);
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
    await expectResponsiveToolbar(composer, contentWidth <= 400);
    await page.evaluate(async () => {
      const fixture =
        await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
      fixture.installVisualComposerEffortFixture({
        levels: ["low", "medium", "extraordinarily high"],
        currentValue: "extraordinarily high",
      });
    });
    await expect(
      composer.getByRole("button", {
        name: "Thinking level: Extraordinarily high",
        exact: true,
      }),
    ).toBeVisible();
    await expectResponsiveToolbar(composer, contentWidth <= 400, false);
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
    await expectResponsiveToolbar(
      composer,
      (await composer.evaluate((element) => element.clientWidth)) <= 400,
    );
  });
}

test.describe("Project chip 200% zoom reflow proxy", () => {
  test.use({ viewport: { width: 480, height: 360 }, deviceScaleFactor: 2 });
  test("keeps the narrow toolbar and editable textarea contained", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    expect(await page.evaluate(() => window.devicePixelRatio)).toBe(2);
    const composer = page.locator("form.composer").first();
    await expectResponsiveToolbar(
      composer,
      await composer.evaluate((element) => element.clientWidth <= 400),
    );
    const textarea = composer.getByRole("textbox", { name: "Message agent" });
    await textarea.fill("Draft remains editable at 200% reflow");
    await expect(textarea).toHaveValue("Draft remains editable at 200% reflow");
    const field = (await textarea.boundingBox())!;
    const form = (await composer.boundingBox())!;
    const toolbar = (await composer
      .locator(".composer__toolbar")
      .boundingBox())!;
    expect(field.height).toBeGreaterThanOrEqual(44);
    expect(field.y).toBeGreaterThanOrEqual(form.y);
    expect(field.y + field.height).toBeLessThanOrEqual(toolbar.y);
    expect(toolbar.y + toolbar.height).toBeLessThanOrEqual(
      form.y + form.height,
    );
    expect(form.y + form.height).toBeLessThanOrEqual(360);
  });
});
