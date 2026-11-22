import { expect, test } from "@playwright/test";
import type { AvailableAcpCommand } from "../../src/services/acpCommandsDesktop";
import { installNativeCommands } from "./fixtures/composerCommands";
import {
  openScenario,
  settleChatProjections,
  waitForAnimations,
} from "./shared/visualBrowserPrimitives";

const commands: AvailableAcpCommand[] = [
  {
    name: "link",
    description: "Attach a link",
    inputHint: "URL",
    source: "builtin",
    placement: { section: "addToMessage" },
  },
  {
    name: "summarize",
    description: "Summarize this session",
    inputHint: null,
    source: "builtin",
    placement: { section: "session" },
  },
  {
    name: "connections",
    description: "Inspect connected servers",
    inputHint: null,
    source: "builtin",
    panel: "mcp",
    placement: { section: "connections", control: "mcpPanel" },
  },
  {
    name: "extensions",
    description: "Manage installed extensions",
    inputHint: null,
    source: "extension",
    placement: { section: "addOns" },
  },
  {
    name: "audit",
    description: "Review a change",
    inputHint: "scope",
    source: "skill",
    placement: { section: "skillsAndCommands" },
  },
  {
    name: "tune",
    description: "Adjust session preferences",
    inputHint: "level",
    source: "builtin",
    placement: { section: "modelAndAccess" },
  },
  {
    name: "status",
    description: "Inspect session usage",
    inputHint: null,
    source: "builtin",
    panel: "status",
    placement: { section: "usage", control: "usagePanel" },
  },
  {
    name: "about",
    description: "About this session",
    inputHint: null,
    source: "builtin",
    placement: { section: "more" },
  },
  {
    name: "deploy",
    description: "Deploy to an environment",
    inputHint: "environment",
    source: "plugin",
    sourceLabel: "Release tools",
    placement: { section: "more" },
  },
];

for (const width of [1888, 720, 480]) {
  test(`shared + sections, search and command insertion at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1096 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const { installVisualComposerActionsFixture } =
        await import("/tests/ui/fixtures/surfaces/composerActionsVisualFixture.ts");
      installVisualComposerActionsFixture([
        { id: "default", name: "Default" },
        { id: "plan", name: "Plan" },
      ]);
    });
    await installNativeCommands(page, commands);
    const composer = page.locator("form.composer").first();
    const trigger = composer.getByRole("button", {
      name: "Composer options",
      exact: true,
    });
    await trigger.click();
    const menu = page.getByRole("menu", { name: "Composer options menu" });
    await expect(
      menu.getByRole("menuitem", { name: "/deploy", exact: true }),
    ).toHaveCount(1);
    const headings = menu.locator(".composer__add-heading");
    await expect(headings).toHaveText([
      "Add to message",
      "Session",
      "Connections",
      "Add-ons",
      "Skills and commands",
      "Model and access",
      "Usage",
      "More",
    ]);
    await expect(
      menu.getByRole("menuitem", { name: "Refresh commands" }),
    ).toHaveCount(0);
    await waitForAnimations(menu);
    await expect(menu).toHaveScreenshot(`composer-plus-menu-${width}.png`);

    // Typing from a menu row enters global search; arrow keys return to rows.
    await page.keyboard.type("deploy");
    const search = menu.getByRole("searchbox", {
      name: "Search composer options",
    });
    await expect(search).toBeFocused();
    await expect(search).toHaveValue("deploy");
    await expect(headings).toHaveText(["More"]);
    await expect(
      menu.getByRole("group", { name: "Plugins · Release tools" }),
    ).toBeVisible();
    await search.fill("More");
    await expect(menu.getByRole("menuitem")).toHaveCount(2);
    if (width === 480) {
      await waitForAnimations(menu);
      await expect(menu).toHaveScreenshot("composer-plus-menu-more-480.png");
    }
    const argumentsField = menu.getByRole("textbox", {
      name: "Arguments for /deploy",
    });
    await search.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Tab");
    await expect(argumentsField).toBeFocused();
    await argumentsField.fill("preview");
    await argumentsField.press("Enter");
    const field = composer.getByRole("combobox", { name: "Message agent" });
    await expect(menu).toHaveCount(0);
    await expect(field).toHaveValue("/deploy preview");
    await expect(field).toBeFocused();

    await field.fill("");
    await trigger.click();
    await menu.getByRole("searchbox").fill("about");
    await menu.getByRole("menuitem", { name: "/about", exact: true }).click();
    await expect(field).toHaveValue("/about ");
    await expect(menu).toHaveCount(0);
    await field.fill("");
    await trigger.click();
    await menu.getByRole("searchbox").fill("status");
    await menu.getByRole("menuitem", { name: "/status", exact: true }).click();
    const panel = page.getByRole("dialog", { name: "Session status" });
    await expect(panel).toBeVisible();
    await expect(menu).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(panel).toHaveCount(0);
    await trigger.click();
    await menu
      .getByRole("menuitem", { name: "MCP servers", exact: true })
      .click();
    const connections = page.getByRole("dialog", { name: "MCP servers" });
    await expect(connections).toBeVisible();
    await expect(menu).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(connections).toHaveCount(0);
  });
}
