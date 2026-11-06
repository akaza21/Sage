import { expect, test, type Locator, type Page } from "./shared/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

/** VS Code's list and tree geometry the Editor tool views follow. */
const LIST_ROW_HEIGHT = 22;
const TREE_INDENT = 8;
const SEARCH_INPUT_HEIGHT = 26;
const COMMIT_FIELD_MIN_HEIGHT = 26;

async function height(locator: Locator) {
  const box = await locator.boundingBox();
  if (!box) throw new Error("element is not rendered");
  return box.height;
}

async function openTool(page: Page, name: string) {
  await page.getByRole("button", { name, exact: true }).click();
  return page.getByLabel(`${name} project tools`, { exact: true });
}

async function guideOpacity(row: Locator) {
  return row.evaluate((element) =>
    Number(getComputedStyle(element, "::before").opacity),
  );
}

async function setActiveFileContent(page: Page, content: string) {
  await page.evaluate(async (next) => {
    const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
    useWorkbenchStore.setState((state) =>
      state.activeEditor?.phase === "ready"
        ? { activeEditor: { ...state.activeEditor, content: next } }
        : {},
    );
  }, content);
}

test.describe("Editor tool views at VS Code list density", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openScenario(page, "editor", "editor");
  });

  test("Explorer rows are 22px, step 8px per level and show guides on hover", async ({
    page,
  }) => {
    const tree = page.getByRole("tree", { name: "Project files" });
    const folder = tree.getByRole("treeitem", { name: /^Toggle folder / });
    const topFolder = folder.first();
    await expect(topFolder).toBeVisible();
    if ((await topFolder.getAttribute("aria-expanded")) !== "true") {
      await topFolder.click();
    }
    const child = topFolder.locator(
      "xpath=following-sibling::*[contains(@class,'tree__row')][1]",
    );

    expect(await height(topFolder)).toBe(LIST_ROW_HEIGHT);
    expect(await height(child)).toBe(LIST_ROW_HEIGHT);
    const [parentIcon, childIcon] = await Promise.all(
      [topFolder, child].map((row) => row.locator("svg").first().boundingBox()),
    );
    expect((childIcon?.x ?? 0) - (parentIcon?.x ?? 0)).toBe(TREE_INDENT);

    await page.mouse.move(0, 0);
    expect(await guideOpacity(child)).toBe(0);
    await child.hover();
    expect(await guideOpacity(child)).toBe(1);
  });

  test("Search controls are 26px and file and match rows are 22px", async ({
    page,
  }) => {
    const search = await openTool(page, "Search");
    const box = search.getByRole("searchbox");
    expect(await height(box.locator(".."))).toBe(SEARCH_INPUT_HEIGHT);
    await box.fill("AgentChatPanes");
    await box.press("Enter");

    const match = search.getByRole("button", {
      name: "Open src/workbench/AgentChatPanes.tsx at line 3, column 17",
    });
    await expect(match).toBeVisible();
    const heading = search.getByRole("button", {
      name: "Collapse results for src/workbench/AgentChatPanes.tsx",
    });
    expect(await height(heading)).toBe(LIST_ROW_HEIGHT);
    expect(await height(match)).toBe(LIST_ROW_HEIGHT);

    // The match text starts under the file's icon, one level in.
    const fileIcon = await heading.locator("svg").nth(1).boundingBox();
    const preview = await match
      .locator(".workspace-search__preview")
      .boundingBox();
    expect(preview?.x).toBe(fileIcon?.x);
    await expect(match.locator(".workspace-search__preview mark")).toHaveText(
      "AgentChatPanes",
    );
  });

  test("Source Control rows are 22px and reveal actions without moving", async ({
    page,
  }) => {
    // A second change that is not the active resource, so its row is not
    // selected and starts with its actions hidden.
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
      useWorkbenchStore.setState((state) => ({
        repositories: {
          ...state.repositories,
          roots: state.repositories.roots.map((repository) => ({
            ...repository,
            changes: [
              ...repository.changes,
              ...repository.changes.slice(0, 1).map((change) => ({
                ...change,
                resource: {
                  ...change.resource,
                  relativePath: "src/workbench/Other.tsx",
                },
              })),
            ],
          })),
        },
      }));
    });
    const source = await openTool(page, "Source Control");
    const discard = source.getByRole("button", {
      name: /Discard unstaged changes to src\/workbench\/Other\.tsx/,
    });
    const row = source.getByRole("button", {
      name: /^Open diff for src\/workbench\/Other\.tsx/,
    });
    await expect(row).toBeVisible();
    expect(await height(row)).toBe(LIST_ROW_HEIGHT);

    await page.mouse.move(0, 0);
    const actions = discard.locator("..");
    await expect(actions).toHaveCSS("opacity", "0");
    const widthAtRest = (await row.boundingBox())?.width;

    await row.hover();
    await expect(actions).toHaveCSS("opacity", "1");
    expect((await row.boundingBox())?.width).toBe(widthAtRest);

    // Keyboard focus reveals the actions too.
    await page.mouse.move(0, 0);
    await discard.focus();
    await expect(actions).toHaveCSS("opacity", "1");
  });

  test("the commit field starts one line tall and grows to a bound", async ({
    page,
  }) => {
    // Stage the fixture's change so the repository offers a commit.
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
      useWorkbenchStore.setState((state) => ({
        repositories: {
          ...state.repositories,
          roots: state.repositories.roots.map((repository) => ({
            ...repository,
            changes: repository.changes.map((change) => ({
              ...change,
              indexStatus: "modified" as const,
            })),
          })),
        },
      }));
    });
    const source = await openTool(page, "Source Control");
    const field = source.getByRole("textbox", { name: /^Commit message/ });
    await expect(field).toBeEnabled();
    expect(await height(field)).toBe(COMMIT_FIELD_MIN_HEIGHT);

    await field.fill("Subject\n\nBody line one\nBody line two");
    const grown = await height(field);
    expect(grown).toBeGreaterThan(COMMIT_FIELD_MIN_HEIGHT);

    await field.fill(Array.from({ length: 40 }, (_, i) => `${i}`).join("\n"));
    const capped = await height(field);
    expect(capped).toBeGreaterThan(grown);
    expect(
      await field.evaluate(
        (element) => element.scrollHeight > element.clientHeight,
      ),
    ).toBe(true);

    await field.fill("");
    expect(await height(field)).toBe(COMMIT_FIELD_MIN_HEIGHT);
  });

  test("Outline nests members as 22px rows that collapse from the keyboard", async ({
    page,
  }) => {
    await setActiveFileContent(
      page,
      "export class Shape {\n  area() {}\n  name = 1;\n}\nconst after = 1;\n",
    );
    const outline = await openTool(page, "Outline");
    const shape = outline.getByRole("treeitem", { name: /^Class: Shape/ });
    const area = outline.getByRole("treeitem", { name: /^Function: area/ });
    await expect(area).toBeVisible();
    expect(await height(shape)).toBe(LIST_ROW_HEIGHT);
    expect(await height(area)).toBe(LIST_ROW_HEIGHT);
    await expect(area).toHaveAttribute("aria-level", "2");

    const [shapeName, areaName] = await Promise.all(
      [shape, area].map((row) =>
        row.locator(".outline-card__name").boundingBox(),
      ),
    );
    expect((areaName?.x ?? 0) - (shapeName?.x ?? 0)).toBe(TREE_INDENT);

    await shape.focus();
    await page.keyboard.press("ArrowLeft");
    await expect(shape).toHaveAttribute("aria-expanded", "false");
    await expect(area).toHaveCount(0);
    await page.keyboard.press("ArrowRight");
    await expect(area).toBeVisible();

    await area.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByTitle("Caret position")).toHaveText("Ln 2, Col 3");
  });

  test("the center is an inset card and the repository summary lines up with its groups", async ({
    page,
  }) => {
    const center = page.locator(".center-region");
    await expect(center).toHaveCSS("border-top-width", "1px");
    await expect(center).toHaveCSS("border-top-left-radius", "6px");
    await expect(center).toHaveCSS("margin-top", "4px");

    const source = await openTool(page, "Source Control");
    const name = source.locator(".source-root__name");
    const branches = source.locator(".source-group-label", {
      hasText: "Branches",
    });
    const [nameBox, labelBox] = await Promise.all([
      name.boundingBox(),
      branches.boundingBox(),
    ]);
    expect(nameBox?.x).toBe(labelBox?.x);
  });

  test("a disabled Commit stays neutral and fully legible", async ({
    page,
  }) => {
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
      useWorkbenchStore.setState((state) => ({
        repositories: {
          ...state.repositories,
          roots: state.repositories.roots.map((repository) => ({
            ...repository,
            changes: repository.changes.map((change) => ({
              ...change,
              indexStatus: "modified" as const,
            })),
          })),
        },
      }));
    });
    const source = await openTool(page, "Source Control");
    const commit = source.locator(
      '.repository-commit__footer button[type="submit"]',
    );
    await expect(commit).toBeDisabled();
    await expect(commit).toHaveCSS("opacity", "1");
    await expect(commit).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  });
});

test("Agent mode keeps its own center and tabs", async ({ page }) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  const center = page.locator(".center-region").first();
  await expect(center).toHaveCSS("border-top-width", "0px");
  await expect(center).toHaveCSS("border-top-left-radius", "0px");
});
