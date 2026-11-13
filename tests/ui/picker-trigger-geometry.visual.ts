import { expect, test, type Locator } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

// Includes the longest release name and both optional development catalog names.
const harnessNames = [
  "Claude Code",
  "Codex",
  "Pi",
  "Cursor Agent",
  "Sage ACP Simulator",
  "Sage Characterization Fixture",
];

async function triggerGeometry(trigger: Locator, labelSelector: string) {
  return trigger.evaluate((element, selector) => {
    const label = element.querySelector(selector)!;
    const range = document.createRange();
    range.selectNodeContents(label);
    const text = range.getBoundingClientRect();
    const labelBox = label.getBoundingClientRect();
    const arrow = element
      .querySelector(":scope > svg:last-child")
      ?.getBoundingClientRect();
    const style = getComputedStyle(element);
    const token = Number.parseFloat(style.getPropertyValue("--space-1"));
    const children = [...element.children]
      .map((child) => child.getBoundingClientRect())
      .filter((box) => box.width > 0 && box.height > 0);
    return {
      gap: arrow ? arrow.left - text.right : 0,
      token,
      width: element.getBoundingClientRect().width,
      minimumWidth: Number.parseFloat(style.minWidth),
      targetFloor: Number.parseFloat(
        style.getPropertyValue("--control-height-compact"),
      ),
      contentWidth:
        children.reduce((sum, box) => sum + box.width, 0) +
        (children.length - 1) * token +
        Number.parseFloat(style.paddingLeft) +
        Number.parseFloat(style.paddingRight) +
        Number.parseFloat(style.borderLeftWidth) +
        Number.parseFloat(style.borderRightWidth),
      labelWidth: labelBox.width,
      textWidth: text.width,
      labelVisible: labelBox.width > 0,
      chevronVisible: Boolean(arrow && arrow.width > 0),
    };
  }, labelSelector);
}

async function expectContentSizedTrigger(
  trigger: Locator,
  labelSelector: string,
) {
  const geometry = await triggerGeometry(trigger, labelSelector);
  // Existing compact hit targets may be wider than an icon-only control.
  // They must never reserve additional label space when the label is visible.
  expect([0, geometry.targetFloor]).toContain(geometry.minimumWidth);
  expect(geometry.width).toBeCloseTo(
    geometry.labelVisible
      ? geometry.contentWidth
      : Math.max(geometry.contentWidth, geometry.minimumWidth),
    1,
  );
  expect(geometry.labelWidth).toBeCloseTo(geometry.textWidth, 1);
  if (geometry.labelVisible && geometry.chevronVisible)
    expect(geometry.gap).toBeCloseTo(geometry.token, 1);
  return geometry;
}

for (const width of [1280, 720]) {
  for (const disabled of [false, true]) {
    test(`shared picker hugs every catalog label at ${width}px (disabled: ${disabled})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await openScenario(page, "agent-single", "agent");
      await page.evaluate(
        async ({ names, disabled }) => {
          const fixture =
            await import("/tests/ui/fixtures/pickerTriggerGeometryFixture.tsx");
          fixture.mountPickerTriggerGeometryFixture(names, disabled);
        },
        { names: harnessNames, disabled },
      );
      const triggers = page.getByRole("region", {
        name: "Shared picker geometry",
      });
      for (const name of harnessNames) {
        const trigger = triggers.getByRole("button", { name, exact: true });
        await expect(trigger).toHaveCSS("display", "flex");
        await expect(trigger).toBeVisible();
        if (disabled) await expect(trigger).toBeDisabled();
        else await expect(trigger).toBeEnabled();
        const geometry = await expectContentSizedTrigger(
          trigger,
          ".searchable-picker__trigger-label",
        );
        expect(geometry.labelVisible).toBe(true);
        expect(geometry.chevronVisible).toBe(true);
        await test.info().attach(name, {
          body: JSON.stringify(geometry),
          contentType: "application/json",
        });
      }
      await expect(triggers.getByRole("listbox")).toHaveCount(0);
    });
  }
}

for (const width of [1280, 960, 480]) {
  for (const scenario of ["agent-single", "error"] as const) {
    test(`composer pickers hug content at ${width}px in ${scenario}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await openScenario(page, scenario, "agent");
      await settleChatProjections(page);
      await page.evaluate(async () => {
        const { useSessionAccessModeStore } =
          await import("/src/app/sessionAccessModeStore.ts");
        useSessionAccessModeStore.setState({
          ready: true,
          modes: { "visual-binding": "readOnly" },
        });
      });
      const composer = page.locator("form.composer");
      const trigger = composer.getByRole("button", {
        name: "Harness",
        exact: true,
      });
      for (const name of harnessNames) {
        await page.evaluate(async (name) => {
          const { useWorkbenchStore } = await import("/src/app/store.ts");
          useWorkbenchStore.setState((state) => ({
            harnesses: state.harnesses.map((harness) => ({ ...harness, name })),
          }));
          await document.fonts.ready;
          await new Promise<void>((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
          );
        }, name);
        await expect(trigger).toHaveText(name);
        const geometry = await expectContentSizedTrigger(
          trigger,
          ".searchable-picker__trigger-label",
        );
        if (width === 1280) {
          expect(geometry.labelVisible).toBe(true);
          expect(geometry.chevronVisible).toBe(true);
        }
        if (!geometry.labelVisible) {
          await expect(trigger).toHaveAccessibleDescription(name);
          await expect(trigger.locator("[data-harness-icon]")).toBeVisible();
          expect(geometry.chevronVisible).toBe(false);
        }
        await test.info().attach(name, {
          body: JSON.stringify(geometry),
          contentType: "application/json",
        });
      }
      for (const [selector, label] of [
        [".composer__model-trigger", ".composer__model-label"],
        [".composer__effort-picker", ".composer__effort-label"],
        [
          ".project-scope .searchable-picker__trigger",
          ".searchable-picker__trigger-label",
        ],
      ]) {
        const control = composer.locator(selector);
        if (await control.count())
          await expectContentSizedTrigger(control, label);
      }
    });
  }
}

for (const width of [1280, 960]) {
  test(`model, effort and project triggers keep the tight gap at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await openScenario(page, "error", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const { installVisualComposerEffortFixture } =
        await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
      installVisualComposerEffortFixture({
        levels: ["low", "high"],
        currentValue: "high",
      });
    });
    const composer = page.locator("form.composer");
    for (const [selector, label] of [
      [".composer__model-trigger", ".composer__model-label"],
      [".composer__effort-picker", ".composer__effort-label"],
      [
        ".project-scope .searchable-picker__trigger",
        ".searchable-picker__trigger-label",
      ],
    ]) {
      const control = composer.locator(selector);
      await expect(control).toBeVisible();
      const geometry = await expectContentSizedTrigger(control, label);
      expect(geometry.labelVisible).toBe(true);
      // A single advertised model keeps the picker trigger and its chevron.
      expect(geometry.chevronVisible).toBe(true);
    }
  });
}
