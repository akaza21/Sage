import { expect, test } from "@playwright/test";
import {
  waitForAnimations,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

const profiles = [
  {
    name: "default-plan",
    modes: [
      { id: "default", name: "Default" },
      { id: "plan", name: "Plan" },
    ],
  },
  {
    name: "build-plan",
    modes: [
      { id: "build", name: "Build" },
      { id: "plan", name: "Plan" },
    ],
  },
  { name: "no-modes", modes: [] },
];

for (const width of [1888, 720, 480]) {
  for (const profile of profiles) {
    test(`CP-29e fixture ${profile.name} Add geometry at ${width}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, "agent-single", "agent");
      await expect(page.locator(".acp-chat-projection").first()).toBeVisible();
      await settleChatProjections(page);
      await page.evaluate(async (modes) => {
        const fixture =
          await import("/tests/ui/fixtures/surfaces/composerActionsVisualFixture.ts");
        fixture.installVisualComposerActionsFixture(modes);
      }, profile.modes);
      const composer = page.locator("form.composer").first();
      const trigger = composer.getByRole("button", {
        name: "Composer options",
        exact: true,
      });
      // Project folds according to the actual composer width, including
      // the responsive navigation state.
      const folded =
        (await composer.getByRole("button", { name: /^Project: / }).count()) ===
        0;
      if (profile.modes.length === 0 && !folded) {
        await expect(trigger).toHaveCount(0);
        await expect(
          page.getByRole("menu", { name: "Composer options menu" }),
        ).toHaveCount(0);
        return;
      }
      await trigger.focus();
      await page.keyboard.press("Enter");
      const menu = page.getByRole("menu", {
        name: "Composer options menu",
        exact: true,
      });
      await expect(menu).toBeVisible();
      await expect(page.locator(".menu-popover:visible")).toHaveCount(1);
      await expect(menu.getByRole("combobox")).toHaveCount(0);
      // Add lists every mode the harness advertises; no id is hidden.
      await expect(
        menu
          .getByRole("menuitemradio")
          .evaluateAll((items) =>
            items.map((item) => item.getAttribute("aria-label")),
          ),
      ).resolves.toEqual(
        profile.modes.map((mode) =>
          mode.name === "Plan" ? "Plan mode" : mode.name,
        ),
      );
      for (const action of ["Files", "Plugins", "Goal", "Add images"]) {
        await expect(
          menu.getByRole("menuitem", { name: action, exact: true }),
        ).toHaveCount(0);
      }
      // Measure the settled panel, not a frame of its enter animation.
      await waitForAnimations(menu);
      const anchor = await trigger.boundingBox();
      const panel = await menu.boundingBox();
      expect(anchor).not.toBeNull();
      expect(panel).not.toBeNull();
      expect(panel!.x).toBeGreaterThanOrEqual(8);
      expect(panel!.x + panel!.width).toBeLessThanOrEqual(width - 8);
      expect(panel!.y).toBeGreaterThanOrEqual(8);
      expect(panel!.y + panel!.height).toBeLessThanOrEqual(1088);
      for (const summary of await page.locator(".acp-file-changes").all())
        await waitForAnimations(summary);
      const edits = await page
        .locator(".chat-edit-summary-zone:not(:empty)")
        .boundingBox();
      const aboveEdge = edits ? Math.min(edits.y, anchor!.y) : anchor!.y;
      expect(
        Math.min(
          Math.abs(panel!.y + panel!.height - aboveEdge),
          Math.abs(panel!.y - anchor!.y - anchor!.height),
        ),
      ).toBeLessThanOrEqual(9);
      expect(
        Math.min(panel!.x + panel!.width, anchor!.x + anchor!.width) -
          Math.max(panel!.x, anchor!.x),
      ).toBeGreaterThan(0);
      if (folded)
        await expect(
          menu.getByRole("menuitem", { name: /^Project: / }),
        ).toBeVisible();
      const items = menu.locator(
        '[role="menuitem"]:enabled, [role="menuitemradio"]:enabled',
      );
      const first = items.first();
      const last = items.last();
      // The current (first) advertised mode, Default included, is the one
      // checked row.
      if (profile.modes.length > 0) {
        await expect(menu.getByRole("menuitemradio").first()).toHaveAttribute(
          "aria-checked",
          "true",
        );
        await expect(
          menu.locator('[role="menuitemradio"][aria-checked="true"]'),
        ).toHaveCount(1);
      }
      await expect(first).toBeFocused();
      await page.keyboard.press("End");
      await expect(last).toBeFocused();
      await page.keyboard.press("Home");
      await expect(first).toBeFocused();
      await page.keyboard.press("Escape");
      await expect(menu).toHaveCount(0);
      await expect(trigger).toBeFocused();
      await trigger.click();
      await composer
        .getByRole("button", { name: "Harness", exact: true })
        .click();
      await expect(menu).toHaveCount(0);
      await expect(page.locator(".menu-popover:visible")).toHaveCount(1);
    });
  }
}
