import { expect, test, type Page } from "./shared/test";

import { editorContent } from "./fixtures/visualDocumentFixtures";
import { openScenario } from "./shared/visualBrowserPrimitives";

// EP-14 change markers in Chromium, against a staged text equal to the File
// as the scenario loads it.
const capture = process.env.SAGE_GUTTER_CAPTURE_DIR;

async function openFile(page: Page) {
  await page.setViewportSize({ width: 1380, height: 880 });
  const workbench = await openScenario(page, "editor", "editor");
  const content = page.locator(".file-surface .cm-content");
  await expect(content).toContainText("export function AgentChatPanes");
  await expect(page.locator(".file-surface .git-gutter")).toBeAttached();
  await page.evaluate(async (staged) => {
    const { getWorkbenchPorts } = await sageImport(
      "/src/ports/workbenchRuntime.ts",
    );
    const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
    getWorkbenchPorts().repository.getFileGitBase = async () => ({
      state: "ready",
      oid: "visual-staged",
      content: staged,
    });
    // A new repository status is what reloads the staged text (EP-12).
    useWorkbenchStore.setState((state) => ({
      repositories: {
        ...state.repositories,
        roots: [...state.repositories.roots.map((root) => ({ ...root }))],
      },
    }));
  }, editorContent);
  await expect(page.locator(".file-surface .git-gutter-marker")).toHaveCount(0);
  return { workbench, content };
}

const line = (page: Page, index: number) =>
  page.locator(".file-surface .cm-line").nth(index);
const markers = (page: Page, kind: string) =>
  page.locator(`.file-surface .git-gutter-marker--${kind}`);

test.describe("Editor change markers", () => {
  test("marks edits, peeks at the staged lines and reverts a change", async ({
    page,
  }) => {
    const { workbench } = await openFile(page);
    await line(page, 0).click();
    await page.keyboard.press("End");
    await page.keyboard.type(" // changed");
    await line(page, 3).click();
    await page.keyboard.press("End");
    await page.keyboard.press("Enter");
    await page.keyboard.type("const added = true;");
    await line(page, 7).click();
    await page.keyboard.press("Home");
    await page.keyboard.press("Home");
    await page.keyboard.press("Shift+ArrowDown");
    await page.keyboard.press("Delete");

    await expect(markers(page, "modified")).toHaveCount(1);
    await expect(markers(page, "added")).toHaveCount(1);
    await expect(markers(page, "deleted-above")).toHaveCount(1);
    if (capture)
      await workbench.screenshot({ path: `${capture}/gutter-markers.png` });

    await markers(page, "modified").first().click();
    const peek = page.getByRole("region", { name: "Change 1 of 3" });
    await expect(peek).toBeVisible();
    await expect(peek.locator(".git-gutter-peek__line")).toHaveCount(1);
    if (capture)
      await workbench.screenshot({ path: `${capture}/gutter-peek.png` });

    await peek.getByRole("button", { name: "Next change" }).click();
    await expect(
      page.getByRole("region", { name: "Change 2 of 3" }),
    ).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator(".git-gutter-peek")).toHaveCount(0);

    await markers(page, "modified").first().click();
    await page
      .getByRole("region", { name: "Change 1 of 3" })
      .getByRole("button", { name: "Revert change" })
      .click();
    await expect(page.locator(".git-gutter-peek")).toHaveCount(0);
    await expect(markers(page, "modified")).toHaveCount(0);
    await expect(line(page, 0)).toHaveText(editorContent.split("\n")[0]!);
    await expect(markers(page, "added")).toHaveCount(1);
    await expect(markers(page, "deleted-above")).toHaveCount(1);
    if (capture)
      await workbench.screenshot({ path: `${capture}/gutter-reverted.png` });

    // The revert is one undo step.
    await page.keyboard.press("ControlOrMeta+z");
    await expect(markers(page, "modified")).toHaveCount(1);
    await expect(line(page, 0)).toContainText("// changed");
  });

  test("turns off with Change markers in Settings", async ({ page }) => {
    await openFile(page);
    await page.evaluate(async () => {
      const { useEditorSettingsStore } = await sageImport(
        "/src/app/editorSettingsStore.ts",
      );
      useEditorSettingsStore.setState((state) => ({
        settings: { ...state.settings, "editor.gitGutter": false },
      }));
    });
    await expect(page.locator(".file-surface .git-gutter")).toHaveCount(0);
  });
});
