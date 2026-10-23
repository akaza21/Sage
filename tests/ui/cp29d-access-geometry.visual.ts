import { expect, test } from "@playwright/test";
import type { SessionAccessModeCapability } from "../../src/domain/sessionAccessMode";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

const readOnly = {
  id: "readOnly" as const,
  label: "Read-only",
  description: "Read workspace files; no writes or network.",
  fallback: true,
};
const workspaceWrite = {
  id: "workspaceWrite" as const,
  label: "Workspace edits",
  description:
    "Edits and commands run without asking inside the workspace and temp folders; no network access.",
};
const ask = {
  id: "ask" as const,
  label: "Ask each time",
  description: "Review each permission request; no filesystem sandbox.",
};
const profiles: { name: string; modes: SessionAccessModeCapability[] }[] = [
  { name: "Codex", modes: [readOnly, workspaceWrite] },
  { name: "Claude", modes: [{ ...ask, fallback: true }] },
  {
    name: "Pi",
    modes: [
      {
        ...readOnly,
        description: "Read-only tools only; workspace editing is not admitted.",
      },
    ],
  },
  {
    name: "OpenCode",
    modes: [
      {
        ...readOnly,
        description: "Sage declines every permission request and file write.",
      },
      {
        ...ask,
        description:
          "Review non-read tool permission requests before execution.",
      },
      {
        ...workspaceWrite,
        description:
          "Auto-approve verified workspace edits; ask for other tools.",
      },
    ],
  },
  { name: "OMP (not admitted)", modes: [] },
];

for (const width of [1888, 720, 480]) {
  for (const profile of profiles) {
    test(`CP-29d ${profile.name} access geometry at ${width}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, "agent-single", "agent");
      await settleChatProjections(page);
      await page.evaluate(async (modes) => {
        const fixture =
          await import("/tests/ui/fixtures/surfaces/composerAccessVisualFixture.ts");
        fixture.installVisualComposerAccessFixture(modes);
      }, profile.modes);
      const composer = page.locator("form.composer").first();
      const trigger = composer.getByRole("button", {
        name: "Access options",
        exact: true,
      });
      const send = composer.getByRole("button", {
        name: "Send message",
        exact: true,
      });
      const accessReason = composer.locator(".composer__disabled-reason", {
        hasText: "Choose a supported access mode before sending.",
      });
      if (profile.modes.length === 0) {
        await expect(trigger).toHaveCount(0);
        await expect(
          page.getByRole("menu", { name: "Access mode" }),
        ).toHaveCount(0);
        await expect(accessReason).toBeVisible();
        await expect(send).toBeDisabled();
        return;
      }
      await expect(accessReason).toHaveCount(0);
      if (profile.name === "Claude") {
        await composer
          .getByRole("textbox", { name: "Message agent", exact: true })
          .fill("Default access sends");
        await expect(send).toBeEnabled();
      }
      // Give Add a capability-backed action and open it first, so the
      // "Access closes Add" assertion below proves a real dismissal.
      await page.evaluate(async () => {
        const fixture =
          await import("/tests/ui/fixtures/surfaces/composerActionsVisualFixture.ts");
        fixture.installVisualComposerActionsFixture([
          { id: "default", name: "Default" },
          { id: "plan", name: "Plan" },
        ]);
      });
      await composer
        .getByRole("button", { name: "Composer options", exact: true })
        .click();
      await expect(
        page.getByRole("menu", { name: "Composer options menu" }),
      ).toBeVisible();
      await trigger.focus();
      await page.keyboard.press("Enter");
      const menu = page.getByRole("menu", { name: "Access mode", exact: true });
      await expect(menu).toBeVisible();
      await expect(
        page.getByRole("menu", { name: "Composer options menu" }),
      ).toHaveCount(0);
      await expect(menu.getByRole("menuitemradio")).toHaveCount(
        profile.modes.length,
      );
      await expect(menu.getByRole("combobox")).toHaveCount(0);
      for (const mode of profile.modes) {
        const option = menu.getByRole("menuitemradio", {
          name: mode.label,
          exact: true,
        });
        await expect(option).toHaveAccessibleDescription(mode.description);
        await expect(
          option.locator(".composer-access-menu__description"),
        ).toBeVisible();
      }
      await expect(menu.getByText("Full access", { exact: true })).toHaveCount(
        0,
      );
      await expect(
        menu.getByText("Approve for me", { exact: true }),
      ).toHaveCount(0);
      // Measure the settled panel, not a frame of its enter animation.
      await menu.evaluate((element) =>
        Promise.all(element.getAnimations().map((motion) => motion.finished)),
      );
      const anchor = await trigger.boundingBox();
      const panel = await menu.boundingBox();
      expect(anchor).not.toBeNull();
      expect(panel).not.toBeNull();
      expect(panel!.x).toBeGreaterThanOrEqual(8);
      expect(panel!.x + panel!.width).toBeLessThanOrEqual(width - 8);
      expect(panel!.y).toBeGreaterThanOrEqual(8);
      expect(panel!.y + panel!.height).toBeLessThanOrEqual(1088);
      expect(
        Math.min(
          Math.abs(panel!.y + panel!.height - anchor!.y),
          Math.abs(panel!.y - anchor!.y - anchor!.height),
        ),
      ).toBeLessThanOrEqual(9);
      expect(
        Math.min(panel!.x + panel!.width, anchor!.x + anchor!.width) -
          Math.max(panel!.x, anchor!.x),
      ).toBeGreaterThan(0);
      const first = menu.getByRole("menuitemradio").first();
      const last = menu.getByRole("menuitemradio").last();
      await page.keyboard.press("End");
      await expect(last).toBeFocused();
      await page.keyboard.press("Home");
      await expect(first).toBeFocused();
      await page.keyboard.press("Escape");
      await expect(menu).toHaveCount(0);
      await expect(trigger).toBeFocused();
    });
  }
}
