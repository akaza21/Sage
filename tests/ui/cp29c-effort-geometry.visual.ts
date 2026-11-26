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
      // The model chip shows the level and opens it below the model list.
      const trigger = page
        .locator("form.composer")
        .first()
        .getByRole("button", { name: "Model", exact: true });
      await expect(trigger.locator(".composer__model-effort")).toHaveText(
        "High",
      );
      await trigger.focus();
      await page.keyboard.press("Enter");
      const panel = page.getByRole("dialog", {
        name: "Select model",
        exact: true,
      });
      const section = panel.getByRole("group", { name: "Thinking level" });
      const slider = section.getByRole("slider", { name: "Thinking level" });
      await expect(panel.getByRole("option", { selected: true })).toBeFocused();
      await expect(slider).toHaveAttribute("min", "0");
      await expect(slider).toHaveAttribute(
        "max",
        String(profile.levels.length - 1),
      );
      await expect(slider).toHaveAttribute("aria-valuetext", "High");
      await expect(section.locator(".composer-effort__stops span")).toHaveCount(
        profile.levels.length,
      );
      await expect(panel.getByRole("combobox")).toHaveCount(0);
      await expect(
        section.getByRole("button", { name: "Reset effort" }),
      ).toHaveCount(profile.defaultValue ? 1 : 0);
      await expect(section.locator("button, input")).toHaveCount(
        profile.defaultValue ? 2 : 1,
      );
      // Tab leaves the list's one stop for the reset, then the slider.
      const stops: string[] = [];
      for (const name of [
        ...(profile.defaultValue ? ["Reset effort"] : []),
        "Thinking level",
      ]) {
        await page.keyboard.press("Tab");
        stops.push(
          await page.evaluate(
            () =>
              document.activeElement?.getAttribute("aria-label") ??
              document.activeElement?.textContent?.trim() ??
              "",
          ),
        );
        expect(stops.at(-1)).toBe(name);
      }
      await expect(slider).toBeFocused();
      await page.keyboard.press("Shift+Tab");
      if (profile.defaultValue)
        await expect(
          section.getByRole("button", { name: "Reset effort" }),
        ).toBeFocused();
      else
        await expect(
          panel.getByRole("option", { selected: true }),
        ).toBeFocused();
      await page.keyboard.press("Tab");
      await expect(slider).toBeFocused();
      const box = (await panel.boundingBox())!;
      const group = (await section.boundingBox())!;
      const rail = (await slider.boundingBox())!;
      const heading = (await section
        .locator(".composer-effort__heading")
        .boundingBox())!;
      const anchor = (await trigger.boundingBox())!;
      const pane = (await page
        .locator(".agent-chat-pane")
        .first()
        .boundingBox())!;
      const expectedWidth = Math.min(232, pane.width, width - 8 - pane.x);
      expect(box.width).toBe(expectedWidth);
      // The level section spans the popover's foot, under the model list.
      expect(group.x).toBeGreaterThanOrEqual(box.x);
      expect(group.x + group.width).toBeLessThanOrEqual(box.x + box.width);
      expect(group.y + group.height).toBeLessThanOrEqual(box.y + box.height);
      expect(group.y).toBeGreaterThan(
        (await panel.getByRole("listbox").boundingBox())!.y,
      );
      expect(rail.height).toBe(24);
      expect(rail.x - group.x).toBe(8);
      expect(rail.width).toBe(group.width - 16);
      expect(rail.y - heading.y - heading.height).toBe(8);
      const centers = await section
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
        section.locator('.composer-effort__stops [data-selected="true"]'),
      ).toHaveCSS("visibility", "hidden");
      expect(box.x).toBeGreaterThanOrEqual(8);
      expect(box.x + box.width).toBeLessThanOrEqual(width - 8);
      expect(box.y).toBeGreaterThanOrEqual(8);
      expect(box.y + box.height).toBeLessThanOrEqual(anchor.y);
      // The popover ends at the chip's end, held inside the composer's dock.
      const dock = (await page
        .locator(".chat-dock-frame")
        .first()
        .boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(pane.x);
      expect(box.x + box.width).toBeLessThanOrEqual(pane.x + pane.width);
      if (width >= 720) {
        expect(box.x).toBeGreaterThanOrEqual(dock.x);
        expect(box.x + box.width).toBeLessThanOrEqual(dock.x + dock.width);
        expect(box.x).toBeCloseTo(
          Math.max(
            dock.x,
            Math.min(
              anchor.x + anchor.width - box.width,
              dock.x + dock.width - box.width,
            ),
          ),
          0,
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
    // Record native calls from here on; every change rejects, so the
    // confirmed level stays authoritative and each attempt is countable. The
    // chat's access reads as its harness default, as outside the desktop, so
    // the live chat's access load never fails into the toolbar.
    const calls: { command: string; value: unknown }[] = [];
    Object.assign(window, { __effortCalls: calls });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: (command: string, args: { request?: { value?: unknown } }) => {
          calls.push({ command, value: args?.request?.value });
          return command === "get_chat_access"
            ? Promise.resolve(null)
            : Promise.reject(new Error("visual fixture"));
        },
        transformCallback: () => 0,
      },
    });
  }, codex);
  const trigger = page
    .locator("form.composer")
    .first()
    .getByRole("button", { name: "Model", exact: true });
  await trigger.focus();
  await page.keyboard.press("Enter");
  const panel = page.getByRole("dialog", { name: "Select model" });
  const slider = panel.getByRole("slider", { name: "Thinking level" });
  await slider.focus();
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
