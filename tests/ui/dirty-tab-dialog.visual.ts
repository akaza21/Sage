import { expect, test } from "./shared/test";
import { openScenario } from "./shared/visualBrowserPrimitives";

test("closing a dirty file loads its confirmation and preserves Cancel and Discard", async ({
  page,
}) => {
  await openScenario(page, "editor", "editor");
  const editor = page.locator(".cm-content");
  await expect(editor).toBeVisible();
  const original = await editor.innerText();
  await editor.click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("// unsaved edit");
  await expect(editor).toContainText("// unsaved edit");
  const tabs = page.getByRole("navigation", { name: "Open tabs" });
  const close = tabs.getByRole("button", { name: /^Close / }).first();
  await close.click();
  const confirmation = page.getByRole("dialog", { name: /^Save changes to / });
  await expect(confirmation).toBeVisible();
  await confirmation
    .getByRole("button", { name: "Cancel", exact: true })
    .click();
  await expect(confirmation).toBeHidden();
  await expect(editor).toContainText("// unsaved edit");
  expect(await editor.innerText()).not.toBe(original);
  await close.click();
  await expect(confirmation).toBeVisible();
  await confirmation
    .getByRole("button", { name: "Discard", exact: true })
    .click();
  await expect(confirmation).toBeHidden();
  await expect(close).toHaveCount(0);
});
