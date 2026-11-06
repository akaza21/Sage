import { expect, test, type Locator, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// What the Editor's selection, search-match and count-badge roles render,
// read from computed styles. Expected values are resolved through a probe
// inside the mode root, so each expectation names a role, not a colour.

type Paint = { fill: string; shadow: string };

function paint(target: Locator): Promise<Paint> {
  return target.evaluate((node) => {
    const style = getComputedStyle(node);
    return { fill: style.backgroundColor, shadow: style.boxShadow };
  });
}

function resolvePaint(page: Page, fill: string, shadow = "none") {
  return page.evaluate(
    ([background, boxShadow]) => {
      const host = document.querySelector(".workbench--editor");
      if (!host) throw new Error("missing .workbench--editor");
      const probe = document.createElement("span");
      probe.style.backgroundColor = background;
      probe.style.boxShadow = boxShadow;
      host.append(probe);
      const style = getComputedStyle(probe);
      const resolved = { fill: style.backgroundColor, shadow: style.boxShadow };
      probe.remove();
      return resolved;
    },
    [fill, shadow] as const,
  );
}

async function hover(page: Page, target: Locator) {
  const box = await target.boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
}

async function selectChange(page: Page) {
  const sourceControl = page.getByRole("button", {
    name: "Source Control",
    exact: true,
  });
  await sourceControl.click();
  const sourceTools = page.getByLabel("Source Control project tools", {
    exact: true,
  });
  const change = sourceTools
    .getByRole("button", {
      name: /^Open diff for src\/workbench\/AgentChatPanes\.tsx/,
    })
    .first();
  // Opening the diff returns the sidebar to Explorer; reopen Source
  // Control, where the change stays selected.
  await change.click();
  await expect(
    page.getByRole("region", {
      name: "Diff for src/workbench/AgentChatPanes.tsx",
    }),
  ).toBeVisible();
  if ((await sourceControl.getAttribute("aria-expanded")) !== "true") {
    await sourceControl.click();
  }
  await expect(change).toHaveClass(/source-change--selected/);
  return { change, sourceTools };
}

test.describe("Editor highlight roles", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  test("keeps selection, search matches and count badges on their roles", async ({
    page,
  }) => {
    await openScenario(page, "editor", "editor");
    const { change, sourceTools } = await selectChange(page);

    const selection = await resolvePaint(
      page,
      "var(--highlight-selection-fill)",
      "inset 1px 0 0 var(--highlight-selection-edge)",
    );
    await page.mouse.move(1, 1);
    expect(await paint(change)).toEqual(selection);
    // Selection stays visible while the pointer rests on the row.
    await hover(page, change);
    expect(await paint(change)).toEqual(selection);

    const countBadge = sourceTools
      .locator(".source-change-group > summary span:last-child")
      .first();
    const quantity = await resolvePaint(page, "var(--color-surface-hover)");
    expect((await paint(countBadge)).fill).toBe(quantity.fill);

    await page.getByRole("button", { name: "Search", exact: true }).click();
    const searchTools = page.getByLabel("Search project tools", {
      exact: true,
    });
    await searchTools.getByRole("searchbox").fill("AgentChatPanes");
    await searchTools.getByRole("searchbox").press("Enter");
    const mark = searchTools.locator(".workspace-search__preview mark").first();
    await expect(mark).toBeVisible();
    expect(await paint(mark)).toEqual(
      await resolvePaint(
        page,
        "var(--editor-find-match-fill)",
        "inset 0 -1px 0 var(--editor-find-match-edge)",
      ),
    );
    const fileCount = searchTools
      .locator(".workspace-search__file-count")
      .first();
    expect((await paint(fileCount)).fill).toBe(quantity.fill);
  });

  test("paints active, pressed and focused controls with the Editor roles", async ({
    page,
  }) => {
    // Read settled paint, not a background transition in flight.
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openScenario(page, "editor", "editor");
    await page.mouse.move(1, 1);

    // Editor-owned rules load last, so they win over the shared tab and
    // title-bar defaults.
    const activeTab = page.locator(".workbench--editor .tab--active").first();
    expect(await paint(activeTab)).toEqual(
      await resolvePaint(
        page,
        "var(--editor-selection-fill)",
        "inset 0 1px 0 var(--editor-selection-edge)",
      ),
    );
    expect(
      await paint(page.locator("header.title-bar .title-bar__mode--active")),
    ).toEqual(
      await resolvePaint(
        page,
        "var(--editor-active-fill)",
        "inset 0 0 0 1px var(--editor-active-edge)",
      ),
    );

    const focusEdge = (await resolvePaint(page, "var(--editor-focus-edge)"))
      .fill;
    const halo = (
      await resolvePaint(page, "transparent", "var(--editor-focus-halo-shadow)")
    ).shadow;
    const field = (target: Locator) =>
      target.evaluate((node) => {
        const style = getComputedStyle(node);
        return { edge: style.borderTopColor, shadow: style.boxShadow };
      });

    await page.locator(".file-surface .cm-content").click();
    await page.keyboard.press("ControlOrMeta+f");
    const find = page.getByRole("search", { name: "Find in file" });
    await expect(
      find.getByRole("textbox", { name: "Find text" }),
    ).toBeFocused();
    // The field draws one edge and halo; its borderless input stays plain.
    expect(await field(find.locator(".editor-find__field").first())).toEqual({
      edge: focusEdge,
      shadow: halo,
    });
    expect(
      (await paint(find.getByRole("textbox", { name: "Find text" }))).shadow,
    ).toBe("none");

    const matchCase = find.getByRole("button", { name: "Match case" });
    await matchCase.click();
    await expect(matchCase).toHaveAttribute("aria-pressed", "true");
    await page.mouse.move(1, 1);
    await find.getByRole("textbox", { name: "Find text" }).focus();
    // Pressed reads as a state, not as keyboard focus.
    expect(await paint(matchCase)).toEqual(
      await resolvePaint(
        page,
        "var(--editor-active-fill)",
        "inset 0 -1px 0 var(--editor-active-edge)",
      ),
    );

    await page.getByRole("button", { name: "Search", exact: true }).click();
    const searchbox = page
      .getByLabel("Search project tools", { exact: true })
      .getByRole("searchbox");
    await searchbox.focus();
    await expect(searchbox).toBeFocused();
    expect(
      await field(
        searchbox.locator(
          "xpath=ancestor::*[contains(@class,'workspace-search__input')][1]",
        ),
      ),
    ).toEqual({ edge: focusEdge, shadow: halo });
  });

  test("outlines the selected change in forced colours", async ({ page }) => {
    // Forced colours drop the selection fill, so the row needs an edge.
    await page.emulateMedia({ forcedColors: "active" });
    await openScenario(page, "editor", "editor");
    const { change } = await selectChange(page);
    await expect(change).toHaveAttribute("aria-current", "true");
    const highlight = await page.evaluate(() => {
      const probe = document.createElement("span");
      probe.style.color = "Highlight";
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    });
    expect(
      await change.evaluate((node) => {
        const style = getComputedStyle(node);
        return [style.outlineStyle, style.outlineWidth, style.outlineColor];
      }),
    ).toEqual(["solid", "1px", highlight]);
  });
});
