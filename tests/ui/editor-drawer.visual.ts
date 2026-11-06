import { expect, test } from "@playwright/test";
import { openScenario } from "./shared/visualBrowserPrimitives";

test.use({ viewport: { width: 720, height: 560 } });

test("opening a file dismisses the compact Editor drawer and reveals the document", async ({
  page,
}) => {
  await openScenario(page, "editor", "editor");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { fileResource } =
      await import("/tests/ui/fixtures/visualDocumentFixtures.ts");
    await useWorkbenchStore.getState().openFile(fileResource);
  });
  const editor = page.getByRole("textbox", {
    name: "Edit src/workbench/AgentChatPanes.tsx",
    exact: true,
  });
  await expect(editor).toBeVisible();
  await expect(editor).toContainText("export function AgentChatPanes");
  await expect(
    page.getByRole("dialog", { name: "Explorer project tools" }),
  ).toBeHidden();
  await expect(
    page.getByRole("tab", { name: /AgentChatPanes.tsx/ }),
  ).toBeVisible();

  await page.getByRole("button", { name: "Explorer", exact: true }).click();
  const drawer = page.getByRole("dialog", { name: "Explorer project tools" });
  await expect(drawer).toBeVisible();
  const center = await page.locator(".center-region").boundingBox();
  const panel = await drawer.boundingBox();
  expect(center).not.toBeNull();
  expect(panel).not.toBeNull();
  expect(panel!.x).toBeLessThanOrEqual(center!.x);
  expect(panel!.y).toBeLessThanOrEqual(center!.y);
  expect(panel!.x + panel!.width).toBeGreaterThanOrEqual(
    center!.x + center!.width,
  );
  expect(panel!.y + panel!.height).toBeGreaterThanOrEqual(
    center!.y + center!.height,
  );
  await expect(editor).toBeHidden();

  await page
    .getByRole("treeitem", {
      name: "Open file src/workbench/AgentChatPanes.tsx, Modified",
      exact: true,
    })
    .click();
  await expect(drawer).toBeHidden();
  await expect(editor).toBeVisible();
  await editor.click();
  await editor.press("End");
  await editor.press("!");
  await expect(editor).toContainText("!");
});
