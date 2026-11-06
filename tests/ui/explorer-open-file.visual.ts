import { expect, test } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// ED-16: in the Editor, Explorer offers Rename and Recycle for the open File.
// The browser runtime has no native file mutations, so both fail here: this
// drives the real UI through the release, the failure, and the File coming
// back where it was. The successful paths are covered by the store tests.
test.describe("Explorer actions on the open File", () => {
  test("offers them, and puts the File back when the move fails", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-content")).toBeVisible();
    const open = await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const state = useWorkbenchStore.getState();
      const tab = state.tabs.find((item) => item.id === state.activeTabId)!;
      // Select the open File in Explorer, as a click on its row does.
      useWorkbenchStore.setState({
        selectedResource: tab.resource,
        selectedRootId: tab.resource!.rootId,
      });
      return { position: tab.position, title: tab.title };
    });
    const more = page.getByRole("button", { name: "More Explorer actions" });
    const menu = page.getByRole("menu", { name: "Explorer actions" });

    await more.click();
    await menu.getByRole("menuitem", { name: "Rename File" }).click();
    const dialog = page.getByRole("dialog", { name: "Rename File" });
    await dialog.getByLabel("File name").fill(`renamed-${open.title}`);
    await dialog.getByRole("button", { name: "Rename" }).click();
    await expect(dialog.getByRole("alert")).toContainText(
      "File rename requires the native Sage desktop",
    );
    await dialog.getByRole("button", { name: "Cancel" }).click();

    // The File is open again, active, at its old place, and loads. (The
    // scenario's content is a fixture, so the reload shows the browser
    // runtime's projection of the File instead.)
    const after = await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const state = useWorkbenchStore.getState();
      const tab = state.tabs.find((item) => item.id === state.activeTabId);
      return { position: tab?.position, title: tab?.title };
    });
    expect(after).toEqual(open);
    await expect(page.locator(".cm-content")).toBeVisible();

    await more.click();
    await menu.getByRole("menuitem", { name: /^Move to/u }).click();
    const confirm = page.getByRole("dialog", { name: /^Move to/u });
    await confirm.getByRole("button", { name: /^Move to/u }).click();
    await expect(confirm.getByRole("alert")).toContainText(
      "requires the native Sage desktop",
    );
    await confirm.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByRole("tab", { name: open.title })).toBeVisible();
    await expect(page.locator(".cm-content")).toBeVisible();
  });
});
