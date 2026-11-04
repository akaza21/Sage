import { expect, test, type Locator, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// Layout behaviour of the Editor chrome, File header, Diff, status bar,
// Settings and Find, proven on the rendered page rather than on CSS text.

type Box = { left: number; right: number; top: number; bottom: number };

const box = (locator: Locator): Promise<Box> =>
  locator.evaluate((node) => {
    const { left, right, top, bottom } = node.getBoundingClientRect();
    return { left, right, top, bottom };
  });

async function openFile(page: Page, relativePath: string) {
  await page.evaluate(async (path) => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    await state.openFile({
      rootId: state.project!.roots[0].id,
      relativePath: path,
    });
  }, relativePath);
}

// Opens the fixture Diff in the Editor, optionally with one extra added line.
async function openDiff(page: Page, extraLine?: string) {
  await page.evaluate(async (text) => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { fileResource, visualDiff } =
      await import("/tests/ui/fixtures/visualDocumentFixtures.ts");
    const { diffCacheKey } = await import("/src/domain/diffIdentity.ts");
    const [hunk] = visualDiff.hunks;
    const value =
      text === undefined
        ? visualDiff
        : {
            ...visualDiff,
            hunks: [
              {
                ...hunk,
                lines: [
                  ...hunk.lines,
                  { kind: "added" as const, oldLine: null, newLine: 22, text },
                ],
              },
            ],
          };
    await useWorkbenchStore.getState().openDiff(fileResource);
    useWorkbenchStore.setState((state) => ({
      diffs: {
        ...state.diffs,
        [diffCacheKey(undefined, fileResource)]: { phase: "ready", value },
      },
    }));
  }, extraLine);
  await expect(page.locator(".diff-line--added").first()).toBeVisible();
}

test.describe("Editor layout invariants", () => {
  test("keeps tab file icons at full size when tab titles shrink (ED-8)", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    await openScenario(page, "editor", "editor");
    const longName = "SomethingVeryLongComponentName.tsx";
    for (const path of ["docs/api.md", "README.md", `src/features/${longName}`])
      await openFile(page, path);
    const longTab = page.locator(`.tab__activate[title$="${longName}"]`);
    await expect(longTab).toBeVisible();
    // The strip is crowded enough that this title is cut short.
    expect(
      await longTab
        .locator("span")
        .first()
        .evaluate((title) => title.scrollWidth > title.clientWidth),
    ).toBe(true);
    const icons = await page
      .locator(".tab__activate > svg")
      .evaluateAll((svgs) =>
        svgs.map((svg) => {
          const { width, height } = svg.getBoundingClientRect();
          return {
            rendered: [width, height],
            intrinsic: [
              Number(svg.getAttribute("width")),
              Number(svg.getAttribute("height")),
            ],
          };
        }),
      );
    expect(icons.length).toBeGreaterThanOrEqual(4);
    for (const icon of icons) expect(icon.rendered).toEqual(icon.intrinsic);
  });

  test("runs the active-line band from the gutter edge to the content's right edge (ED-10)", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await page.locator(".file-surface .cm-line").nth(2).click();
    const band = await box(page.locator(".file-surface .cm-activeLine"));
    const gutters = await box(page.locator(".file-surface .cm-gutters"));
    const contentRight = await page
      .locator(".file-surface .cm-scroller")
      .evaluate(
        (scroller) =>
          scroller.getBoundingClientRect().left +
          scroller.clientLeft +
          scroller.clientWidth,
      );
    expect(band.left).toBeCloseTo(gutters.right, 1);
    expect(band.right).toBeCloseTo(contentRight, 1);
  });

  test("keeps the File header metadata at the right with or without actions (ED-13)", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    const header = page.locator(".file-surface .file-header");
    const metadata = header.locator(".file-header__metadata");
    const actions = header.locator(".file-header__actions");
    const inner = await header.evaluate((node) => {
      const style = getComputedStyle(node);
      return {
        right:
          node.getBoundingClientRect().right -
          parseFloat(style.borderRightWidth) -
          parseFloat(style.paddingRight),
        gap: parseFloat(style.columnGap),
      };
    });
    await expect(actions).toHaveCount(0);
    expect((await box(metadata)).right).toBeCloseTo(inner.right, 1);

    // An edit adds the unsaved-state action after the metadata.
    await page.locator(".file-surface .cm-line").nth(2).click();
    await page.keyboard.type("x");
    await expect(actions).toBeVisible();
    const actionsBox = await box(actions);
    expect(actionsBox.right).toBeCloseTo(inner.right, 1);
    expect((await box(metadata)).right + inner.gap).toBeCloseTo(
      actionsBox.left,
      1,
    );
  });

  test("fits a short Diff to a narrow pane and keeps line numbers in view on long lines", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 960, height: 640 });
    await openScenario(page, "editor", "editor");
    await openDiff(page);
    const surface = page.locator(".diff-surface");
    const editor = surface.locator(".diff-editor");
    const surfaceBox = await box(surface);
    const short = await editor.evaluate((node) => ({
      scrolls: node.scrollWidth > node.clientWidth,
      rowRights: [...node.querySelectorAll(".diff-line")].map(
        (row) => row.getBoundingClientRect().right,
      ),
    }));
    expect(short.scrolls).toBe(false);
    for (const right of short.rowRights)
      expect(right).toBeLessThanOrEqual(surfaceBox.right);

    await openDiff(page, `const wide = "${"x".repeat(400)}";`);
    const scrolled = await editor.evaluate((node) => {
      const start = node.getBoundingClientRect().left + node.clientLeft;
      const row = node.querySelector(".diff-line--added")!;
      const hunkLabel = node.querySelector(".diff-hunk__header > span")!;
      const before = {
        numbers: [...row.querySelectorAll(":scope > span")].map(
          (cell) => cell.getBoundingClientRect().left - start,
        ),
        hunk: hunkLabel.getBoundingClientRect().left - start,
      };
      node.scrollLeft = node.scrollWidth;
      return {
        scrollLeft: node.scrollLeft,
        before,
        after: {
          numbers: [...row.querySelectorAll(":scope > span")].map(
            (cell) => cell.getBoundingClientRect().left - start,
          ),
          hunk: hunkLabel.getBoundingClientRect().left - start,
        },
      };
    });
    expect(scrolled.scrollLeft).toBeGreaterThan(0);
    expect(scrolled.before.numbers).toHaveLength(2);
    expect(scrolled.after).toEqual(scrolled.before);
  });

  test("paints the Diff hunk header unlike an added line", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await openDiff(page);
    const colours = await page.locator(".diff-surface").evaluate((surface) => {
      const style = (selector: string) =>
        getComputedStyle(surface.querySelector(selector)!);
      return {
        hunkText: style(".diff-hunk__header").color,
        hunkFill: style(".diff-hunk__header").backgroundColor,
        addedFill: style(".diff-line--added").backgroundColor,
        // The header's "+N" count is drawn in the added colour.
        addedText: style(".diff-header__counts span:first-child").color,
      };
    });
    expect(colours.hunkFill).not.toBe(colours.addedFill);
    expect(colours.hunkText).not.toBe(colours.addedText);
  });

  test("dims Settings pages with no match for the query", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await page.getByRole("button", { name: "Open settings" }).click();
    const settings = page.getByRole("region", {
      name: "Settings",
      exact: true,
    });
    await settings.getByRole("searchbox").fill("terminal");
    const pages = settings.getByRole("tablist").getByRole("tab");
    const opacity = (name: string) =>
      pages
        .filter({ hasText: name })
        .evaluate((tab) => Number(getComputedStyle(tab).opacity));
    await expect(pages.filter({ hasText: "Browser" })).toHaveAttribute(
      "data-matches",
      "false",
    );
    await expect(pages.filter({ hasText: "Keyboard" })).toHaveAttribute(
      "data-matches",
      "true",
    );
    expect(await opacity("Browser")).toBeLessThan(await opacity("Keyboard"));
    await settings.getByRole("searchbox").fill("");
    expect(await opacity("Browser")).toBe(await opacity("Keyboard"));
  });

  test("lays alternate chords for one command side by side without touching", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 480, height: 880 });
    await openScenario(page, "agent-single", "agent");
    await page.keyboard.press("Control+,");
    const settings = page.getByRole("dialog", { name: "Agent settings" });
    await settings
      .getByRole("navigation", { name: "Agent settings sections" })
      .getByRole("button", { name: "Keybindings", exact: true })
      .click();
    const chords = settings
      .getByRole("row", { name: /^Send message/ })
      .locator("kbd");
    await expect(chords).toHaveText(["Enter", "Ctrl+Enter"]);
    const [first, second] = [
      await box(chords.nth(0)),
      await box(chords.nth(1)),
    ];
    expect(second.left).toBeGreaterThan(first.right);
    expect(second.top).toBeCloseTo(first.top, 1);
  });

  for (const width of [480, 1380]) {
    test(`keeps Find right-aligned inside the editor stage at ${width}px`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 640 });
      await openScenario(page, "editor", "editor");
      // A narrow window opens the tool sidebar as a drawer over the editor.
      const hideSidebar = page.getByRole("button", {
        name: "Hide left sidebar",
      });
      if (await hideSidebar.isVisible()) await hideSidebar.click();
      await page.locator(".file-surface .cm-content").click();
      await page.keyboard.press("ControlOrMeta+f");
      const find = page.getByRole("search", { name: "Find in file" });
      await expect(find).toBeVisible();
      const findBox = await box(find);
      const stage = await box(page.locator(".file-editor-stage"));
      expect(findBox.left).toBeGreaterThanOrEqual(stage.left);
      expect(findBox.right).toBeLessThanOrEqual(stage.right);
      expect(findBox.top).toBeGreaterThanOrEqual(stage.top);
      expect(findBox.bottom).toBeLessThanOrEqual(stage.bottom);
      expect(stage.right - findBox.right).toBeLessThanOrEqual(
        findBox.left - stage.left,
      );
      expect(
        await find.evaluate((node) => node.scrollWidth <= node.clientWidth),
      ).toBe(true);
    });
  }
});
