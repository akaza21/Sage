import { expect, test, type Page } from "@playwright/test";
import { expectCompactComposerTargets } from "./shared/composerGeometry";
import {
  openScenario,
  settleChatProjections,
  expectNoViewportOverflow,
} from "./shared/visualBrowserPrimitives";

const profiles = [
  {
    name: "Codex",
    id: "effort",
    levels: ["minimal", "low", "medium", "high", "xhigh"],
    currentValue: "high",
    defaultValue: "medium",
    centers: [12, 57.5, 103, 148.5, 194],
  },
  {
    name: "Claude",
    id: "effort",
    levels: ["low", "medium", "high"],
    currentValue: "high",
    centers: [12, 103, 194],
  },
  {
    name: "Pi",
    id: "pi.thinking",
    levels: ["off", "high"],
    currentValue: "high",
    centers: [12, 194],
  },
];

for (const width of [1888, 720, 480]) {
  for (const profile of profiles) {
    test(`CP-29c ${profile.name} effort geometry at ${width}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, "agent-single", "agent");
      await settleChatProjections(page);
      await page.evaluate(async (config) => {
        const fixture =
          await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
        fixture.installVisualComposerEffortFixture(config);
      }, profile);
      if (width === 480)
        await expectCompactComposerTargets(
          page.locator("form.composer").first(),
        );
      const trigger = page
        .locator("form.composer")
        .first()
        .getByRole("button", { name: "High", exact: true });
      await trigger.focus();
      await page.keyboard.press("Enter");
      const panel = page.getByRole("dialog", {
        name: "Thinking level",
        exact: true,
      });
      const slider = panel.getByRole("slider", { name: "Thinking level" });
      await expect(slider).toBeFocused();
      await expect(slider).toHaveAttribute("min", "0");
      await expect(slider).toHaveAttribute(
        "max",
        String(profile.levels.length - 1),
      );
      await expect(slider).toHaveAttribute("aria-valuetext", "High");
      await expect(panel.locator(".composer-effort__stops span")).toHaveCount(
        profile.levels.length,
      );
      await expect(panel.getByRole("combobox")).toHaveCount(0);
      await expect(
        panel.getByRole("button", { name: "Reset effort" }),
      ).toHaveCount(profile.defaultValue ? 1 : 0);
      await expect(
        panel.getByRole("button", { name: "Select model: GPT-6 Astra" }),
      ).toBeVisible();
      await expect(panel.locator("button, input")).toHaveCount(
        profile.defaultValue ? 3 : 2,
      );
      expect(
        await panel
          .locator("button, input")
          .evaluateAll((controls) =>
            controls.map((control) => control.getAttribute("aria-label")),
          ),
      ).toEqual([
        "Select model: GPT-6 Astra",
        ...(profile.defaultValue ? ["Reset effort"] : []),
        "Thinking level",
      ]);
      await page.keyboard.press("Shift+Tab");
      await expect(
        panel.getByRole("button", {
          name: profile.defaultValue
            ? "Reset effort"
            : "Select model: GPT-6 Astra",
        }),
      ).toBeFocused();
      await page.keyboard.press("Tab");
      await expect(slider).toBeFocused();
      const box = (await panel.boundingBox())!;
      const rail = (await slider.boundingBox())!;
      const heading = (await panel
        .locator(".composer-effort__heading")
        .boundingBox())!;
      const anchor = (await trigger.boundingBox())!;
      const pane = (await page
        .locator(".agent-chat-pane")
        .first()
        .boundingBox())!;
      const expectedWidth = Math.min(232, pane.width, width - 8 - pane.x);
      expect(box.width).toBe(expectedWidth);
      expect(box.height).toBeGreaterThanOrEqual(84);
      expect(box.height).toBeLessThanOrEqual(90);
      expect(rail.height).toBe(24);
      expect(rail.width).toBe(expectedWidth - 26);
      expect(rail.x - box.x).toBe(13);
      expect(rail.y - heading.y - heading.height).toBe(8);
      const centers = await panel
        .locator(".composer-effort__stops span")
        .evaluateAll((stops) =>
          stops.map((stop) => {
            const rect = stop.getBoundingClientRect();
            return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
          }),
        );
      for (const [index, center] of centers.entries()) {
        expect(center.x - rail.x).toBeCloseTo(
          12 + (index * (rail.width - 24)) / (profile.levels.length - 1),
          1,
        );
        expect(center.y - rail.y).toBe(12);
      }
      await expect(
        panel.locator('.composer-effort__stops [data-selected="true"]'),
      ).toHaveCSS("visibility", "hidden");
      expect(box.x).toBeGreaterThanOrEqual(8);
      expect(box.x + box.width).toBeLessThanOrEqual(width - 8);
      expect(box.y).toBeGreaterThanOrEqual(8);
      expect(box.y + box.height).toBeLessThanOrEqual(anchor.y);
      const composer = (await page
        .locator("form.composer")
        .first()
        .boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(pane.x);
      expect(box.x + box.width).toBeLessThanOrEqual(pane.x + pane.width);
      if (width >= 720) {
        expect(box.x).toBeGreaterThanOrEqual(composer.x);
        expect(box.x + box.width).toBeLessThanOrEqual(
          composer.x + composer.width,
        );
      }
      await expectNoViewportOverflow(page);
      await page.keyboard.press("Escape");
      await expect(panel).toHaveCount(0);
      await expect(trigger).toBeFocused();
    });
  }
}

const codex = profiles[0];

async function openCodexEffort(page: Page) {
  await page.setViewportSize({ width: 720, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async (config) => {
    const fixture =
      await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
    fixture.installVisualComposerEffortFixture(config);
    // Record effort mutations from here on; every native call rejects, so the
    // confirmed level stays authoritative and each attempt is countable.
    const calls: { command: string; value: unknown }[] = [];
    Object.assign(window, { __effortCalls: calls });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: (command: string, args: { request?: { value?: unknown } }) => {
          calls.push({ command, value: args?.request?.value });
          return Promise.reject(new Error("visual fixture"));
        },
        transformCallback: () => 0,
      },
    });
  }, codex);
  const trigger = page
    .locator("form.composer")
    .first()
    .getByRole("button", { name: "High", exact: true });
  await trigger.focus();
  await page.keyboard.press("Enter");
  const panel = page.getByRole("dialog", { name: "Thinking level" });
  const slider = panel.getByRole("slider", { name: "Thinking level" });
  await expect(slider).toBeFocused();
  return { panel, slider };
}

function effortCalls(page: Page) {
  return page.evaluate(() =>
    (
      window as unknown as {
        __effortCalls: { command: string; value: unknown }[];
      }
    ).__effortCalls.filter(
      (call) => call.command === "set_acp_session_config_option",
    ),
  );
}

test("CP-29c keyboard steps one advertised level per key", async ({ page }) => {
  const { slider } = await openCodexEffort(page);
  const expected: Record<string, string> = {
    ArrowLeft: "medium",
    ArrowDown: "medium",
    ArrowRight: "xhigh",
    ArrowUp: "xhigh",
    Home: "minimal",
    End: "xhigh",
  };
  for (const [key, value] of Object.entries(expected)) {
    const before = (await effortCalls(page)).length;
    await page.keyboard.press(key);
    await expect
      .poll(async () => (await effortCalls(page)).length)
      .toBe(before + 1);
    expect((await effortCalls(page)).at(-1)?.value).toEqual({ value });
    await expect(slider).toHaveAttribute("aria-valuetext", "High");
    await expect(slider).toBeFocused();
  }
});

test("CP-29c drag across stops snaps to advertised levels and commits once", async ({
  page,
}) => {
  const { panel, slider } = await openCodexEffort(page);
  const rail = (await slider.boundingBox())!;
  const y = rail.y + 12;
  await page.mouse.move(rail.x + codex.centers[3], y);
  await page.mouse.down();
  const names = ["Minimal", "Low", "Medium", "High", "Xhigh"];
  for (const x of [130, 103, 80, 57.5, 30, 12]) {
    await page.mouse.move(rail.x + x, y, { steps: 3 });
    const text = await slider.getAttribute("aria-valuetext");
    expect(names).toContain(text);
    expect(Number(await slider.inputValue()) % 1).toBe(0);
  }
  await expect(slider).toHaveAttribute("aria-valuetext", "Minimal");
  expect(await effortCalls(page)).toHaveLength(0);
  await page.mouse.up();
  await expect.poll(async () => (await effortCalls(page)).length).toBe(1);
  expect((await effortCalls(page))[0].value).toEqual({ value: "minimal" });
  await expect(panel).toBeVisible();
  await expect(slider).toHaveAttribute("aria-valuetext", "High");
  await slider.blur();
  expect(await effortCalls(page)).toHaveLength(1);
});
