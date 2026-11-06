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

test.describe("Editor highlight roles", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  test("keeps selection, search matches and count badges on their roles", async ({
    page,
  }) => {
    await openScenario(page, "editor", "editor");

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
});
