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

test("failed Reload keeps edits and exposes bounded native diagnostics only in Details", async ({
  page,
}) => {
  await openFile(page);
  await typeAtEnd(page, "// keep these edits");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    await useWorkbenchStore.getState().flushActiveEditor();
    const { getWorkbenchPorts } =
      await import("/src/ports/workbenchRuntime.ts");
    getWorkbenchPorts().editor.discardEditorBuffer = async () => {
      throw new Error(
        "filesystem operation failed: The process cannot access the file because it is being used by another process. (os error 32) " +
          "diagnostic ".repeat(200),
      );
    };
  });
  await changeOnDisk(page, "// changed elsewhere\n");
  const conflict = page.getByRole("group", { name: "External change" });
  await conflict.getByRole("button", { name: "Reload" }).click();
  const failure = page.getByRole("alert");
  await expect(failure).toContainText("Could not reload this file.");
  await expect(failure).not.toContainText("filesystem operation failed");
  await expect(conflict).toBeVisible();
  await expect(page.locator(".cm-content")).toContainText(
    "// keep these edits",
  );
  await failure.getByRole("button", { name: "Details" }).click();
  const details = failure.locator(".notice__details");
  await expect(details).toContainText("filesystem operation failed:");
  expect((await details.textContent())?.length).toBe(1024);
  await expect(details).toHaveAttribute("aria-live", "off");
});

for (const width of [1280, 720]) {
  test(`resolves a File changed on disk at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await openFile(page);
    const banner = page.getByRole("group", { name: "External change" });

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
    // Back in the editor, so the save is one keystroke away.
    await expect(page.locator(".cm-content")).toBeFocused();
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
    const banner = page.getByRole("group", { name: "Deleted file" });

    // Keep mine, then save: the File comes back with the buffer.
    await typeAtEnd(page, "// kept");
    await changeOnDisk(page, null);
    await expect(banner).toBeVisible();
    await expect(banner.getByRole("button", { name: "Reload" })).toHaveCount(0);
    await shot(page, "deleted");
    // Explorer lets go of the File too.
    await expect(
      page
        .getByRole("tree", { name: "Project files" })
        .getByRole("treeitem", { name: /Workbench\.tsx/u }),
    ).toHaveCount(0);
    await expect(page.locator(".file-header__disk")).toBeHidden();
    await banner.getByRole("button", { name: "Keep mine" }).click();
    await expect(banner).toBeHidden();
    const pending = page.getByRole("group", { name: "Kept version" });
    await expect(pending).toContainText("saving creates it again");
    await expect(pending).toBeVisible();
    await expect(pending.getByRole("button", { name: "Close" })).toBeVisible();
    await shot(page, "deleted-kept");
    await expect(page.locator(".cm-content")).toBeFocused();
    await page.keyboard.press("ControlOrMeta+s");
    await expect(pending).toBeHidden();
    await expect(page.locator(".file-header__disk")).toBeVisible();
    expect(await diskText(page)).toContain("// kept");

    // Close drops the edit and the tab.
    await typeAtEnd(page, "// gone");
    await changeOnDisk(page, null);
    await banner.getByRole("button", { name: "Close" }).click();
    await expect(
      page.getByRole("tab", { name: /Workbench\.tsx/u }),
    ).toHaveCount(0);
  });

  test(`says a deleted File is not watched at the limit at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await openFile(page);
    // Every watch slot is taken from here on.
    await page.evaluate(async () => {
      const { getWorkbenchPorts } =
        await import("/src/ports/workbenchRuntime.ts");
      const { EDITOR_WATCH_LIMIT_ERROR } =
        await import("/src/features/files/editorExternalWatch.ts");
      getWorkbenchPorts().editor.watchEditorDocument = async () => {
        throw EDITOR_WATCH_LIMIT_ERROR;
      };
    });
    await typeAtEnd(page, "// kept");
    await changeOnDisk(page, null);
    await expect(
      page.getByRole("group", { name: "Deleted file" }),
    ).toBeVisible();
    // The size goes with the File; why it is unwatched stays in view.
    await expect(page.locator(".file-header__disk")).toBeHidden();
    const unwatched = page.locator(".file-header__unwatched");
    await expect(unwatched).toBeVisible();
    await expect(unwatched).toHaveText("· Not watched: watch limit reached");
    await shot(page, "deleted-watch-limit");
  });
}

test("Revert File in the command palette drops unsaved edits", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openFile(page);
  const onDisk = await diskText(page);
  await typeAtEnd(page, "// to revert");
  const dialog = page.getByRole("dialog", { name: "Quick Open" });
  const box = dialog.getByRole("combobox", { name: "Run a command" });

  await page.keyboard.press("ControlOrMeta+Shift+p");
  await box.fill(">revert file");
  await expect(dialog.getByRole("option")).toHaveCount(1);
  await box.press("Enter");

  await expect(dialog).toBeHidden();
  await expect(page.locator(".cm-content")).not.toContainText("// to revert");
  expect(await diskText(page)).toBe(onDisk);
  // With nothing unsaved, there is nothing to revert.
  await page.keyboard.press("ControlOrMeta+Shift+p");
  await box.fill(">revert file");
  await expect(dialog.getByRole("option")).toHaveCount(0);
});
