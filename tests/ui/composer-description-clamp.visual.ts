import { expect, test, type Locator, type Page } from "@playwright/test";
import type { AvailableAcpCommand } from "../../src/services/acpCommandsDesktop";
import { installNativeCommands } from "./fixtures/composerCommands";
import {
  openScenario,
  settleChatProjections,
  waitForAnimations,
} from "./shared/visualBrowserPrimitives";

// A skill description as long as the ones harnesses really advertise.
const longDescription = Array.from(
  { length: 12 },
  (_, index) => `Step ${index + 1} explains when this skill applies.`,
).join(" ");

const skill: AvailableAcpCommand = {
  name: "long-skill",
  description: longDescription,
  inputHint: null,
  source: "skill",
  placement: { section: "skillsAndCommands" },
};

async function openMenu(page: Page, height: number) {
  await page.setViewportSize({ width: 1280, height });
  await openScenario(page, "agent-composer-goal", "agent");
  await settleChatProjections(page);
  await installNativeCommands(page, [skill]);
  await page
    .locator("form.composer")
    .first()
    .getByRole("button", { name: "Composer options", exact: true })
    .click();
  const menu = page.getByRole("menu", { name: "Composer options menu" });
  await expect(
    menu.getByRole("menuitem", { name: "/long-skill", exact: true }),
  ).toBeVisible();
  await waitForAnimations(menu);
  return menu;
}

const overflow = (menu: Locator) =>
  menu.evaluate((element) => element.scrollHeight - element.clientHeight);
const thumb = (menu: Locator) =>
  menu.evaluate((element) => getComputedStyle(element).scrollbarColor);

/** Two lines at most, cut with an ellipsis, and the whole text on hover. */
async function expectClamped(description: Locator) {
  await expect(description).toHaveAttribute("title", longDescription);
  const box = await description.evaluate((element) => ({
    height: element.getBoundingClientRect().height,
    line: parseFloat(getComputedStyle(element).lineHeight),
    cut: element.scrollHeight > element.clientHeight,
  }));
  expect(box.height).toBeLessThanOrEqual(box.line * 2 + 1);
  expect(box.cut).toBe(true);
}

test("the + menu fits its rows and a skill without a scroll, clamping the skill's description", async ({
  page,
}) => {
  const menu = await openMenu(page, 900);
  expect(await overflow(menu)).toBeLessThanOrEqual(0);
  const row = menu.getByRole("menuitem", { name: "/long-skill", exact: true });
  await expect(row).toHaveAttribute("aria-description", longDescription);
  await expectClamped(row.locator(".menu-description"));
});

test("a + menu taller than a short window scrolls with its bar shown only on hover", async ({
  page,
}) => {
  const menu = await openMenu(page, 420);
  expect(await overflow(menu)).toBeGreaterThan(0);
  // The click that opened it may have left the pointer over the menu.
  await page.mouse.move(0, 0);
  const hidden = await thumb(menu);
  expect(hidden).toBe("rgba(0, 0, 0, 0) rgba(0, 0, 0, 0)");
  await menu.hover();
  expect(await thumb(menu)).toBe("rgb(58, 58, 58) rgba(0, 0, 0, 0)");
});

test("the / picker clamps a long description and keeps it whole for assistive tech", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "agent-composer-goal", "agent");
  await settleChatProjections(page);
  await installNativeCommands(page, [skill]);
  await page
    .locator("form.composer")
    .first()
    .getByRole("combobox", { name: "Message agent" })
    .focus();
  await page.keyboard.type("/");
  const option = page
    .getByRole("listbox", { name: "Slash commands" })
    .getByRole("option", { name: `/long-skill, ${longDescription}` });
  await expect(option).toBeVisible();
  await expectClamped(option.locator(".menu-description"));
});
