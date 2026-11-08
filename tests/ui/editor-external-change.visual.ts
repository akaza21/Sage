import { expect, test, type Page } from "@playwright/test";

import { installLayoutMode } from "./shared/visualBrowserPrimitives";

// #235: the banner that resolves a File changed or deleted outside Sage,
// driven against the browser runtime's in-memory files. `changeOnDisk` stands
// in for another program writing or deleting the File; the File's watch hears
// it as the native watch would.
const FILE = {
  rootId: "browser-root",
  relativePath: "src/workbench/Workbench.tsx",
};

// Set to a folder to keep a picture of each banner state.
const SHOT_DIR = process.env.SAGE_BANNER_SHOT_DIR;

async function openFile(page: Page) {
  await installLayoutMode(page, "editor");
  await page.goto("/");
  await expect(page.locator("main.workbench")).toBeVisible();
  await page.evaluate(async (file) => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    await useWorkbenchStore.getState().openFile(file);
  }, FILE);
  await expect(page.locator(".cm-content")).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
}

async function typeAtEnd(page: Page, text: string) {
  await page.locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type(text);
  await expect(page.locator(".cm-content")).toContainText(text);
}

function changeOnDisk(page: Page, content: string | null) {
  return page.evaluate(
    async ({ file, content }) => {
      const { browserEditor } = await import("/src/services/browserEditor.ts");
      browserEditor.changeOnDisk(file, content);
    },
    { file: FILE, content },
  );
}

function diskText(page: Page) {
  return page.evaluate(async (file) => {
    const { browserEditor } = await import("/src/services/browserEditor.ts");
    return browserEditor.readTextFile(file).content;
  }, FILE);
}

async function shot(page: Page, name: string) {
  if (!SHOT_DIR) return;
  const width = page.viewportSize()?.width;
  await page.screenshot({ path: `${SHOT_DIR}/${name}-${width}.png` });
}

for (const width of [1280, 720]) {
  test(`resolves a File changed on disk at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await openFile(page);
    const banner = page.getByRole("alert").filter({
      hasText: "External change detected.",
    });

    // Compare, then Keep mine: the save writes the buffer over the change.
    await typeAtEnd(page, "// mine");
    await changeOnDisk(page, "// written elsewhere\n");
    await expect(banner).toBeVisible();
    await expect(
      page.getByRole("tab", { name: /Workbench\.tsx/u }),
    ).toBeVisible();
    await shot(page, "changed");
    await banner.getByRole("button", { name: "Compare" }).click();
    const compare = page.getByRole("region", {
      name: /on disk with your version/u,
    });
    await expect(compare.locator(".diff-line--removed")).toContainText(
      "// written elsewhere",
    );
    await expect(compare.locator(".diff-line--added").last()).toContainText(
      "// mine",
    );
    await shot(page, "compare");
    await banner.getByRole("button", { name: "Keep mine" }).click();
    await expect(banner).toBeHidden();
    await expect(compare).toBeHidden();
    await page.locator(".cm-content").click();
    await page.keyboard.press("ControlOrMeta+s");
    await expect.poll(() => diskText(page)).toContain("// mine");
    expect(await diskText(page)).not.toContain("// written elsewhere");

    // Reload takes the File on disk and drops the edit.
    await typeAtEnd(page, "// dropped");
    await changeOnDisk(page, "// reloaded from disk\n");
    await banner.getByRole("button", { name: "Reload" }).click();
    await expect(banner).toBeHidden();
    await expect(page.locator(".cm-content")).toHaveText(
      "// reloaded from disk",
    );
  });

  test(`resolves a File deleted on disk at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await openFile(page);
    const banner = page.getByRole("alert").filter({ hasText: "File deleted." });

    // Keep mine, then save: the File comes back with the buffer.
    await typeAtEnd(page, "// kept");
    await changeOnDisk(page, null);
    await expect(banner).toBeVisible();
    await expect(banner.getByRole("button", { name: "Reload" })).toHaveCount(0);
    await shot(page, "deleted");
    await banner.getByRole("button", { name: "Keep mine" }).click();
    await expect(banner).toBeHidden();
    const pending = page
      .getByRole("status")
      .filter({ hasText: "Save it to create it again" });
    await expect(pending).toBeVisible();
    await shot(page, "deleted-kept");
    await page.locator(".cm-content").click();
    await page.keyboard.press("ControlOrMeta+s");
    await expect(pending).toBeHidden();
    expect(await diskText(page)).toContain("// kept");

    // Close drops the edit and the tab.
    await typeAtEnd(page, "// gone");
    await changeOnDisk(page, null);
    await banner.getByRole("button", { name: "Close" }).click();
    await expect(
      page.getByRole("tab", { name: /Workbench\.tsx/u }),
    ).toHaveCount(0);
  });
}
