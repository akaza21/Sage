import { expect, test } from "@playwright/test";

test("mixed resource and browser tabs share a 560 px pane without clipping controls", async ({
  page,
}) => {
  test.setTimeout(60_000);
  await page.setViewportSize({ width: 1920, height: 900 });
  await page.setContent(
    `
    <div class="workbench--agent">
      <div class="workbench__body" style="width: 1920px; display: block">
        <aside class="agent-artifact-pane" style="width: 560px">
          <header class="agent-artifact-pane__header">
            <nav class="tab-strip" aria-label="Agent artifact tabs">
              <div class="tab-strip__scroll">
                <div class="tab" data-resource-kind="file"><button class="tab__activate" aria-label="File src/SettingsPanel.tsx" title="File src/SettingsPanel.tsx"><svg width="16" height="16" viewBox="0 0 16 16"></svg><span class="tab__path"><span class="tab__dir">src/</span><span class="tab__basename">SettingsPanel.tsx</span></span></button><button class="tab__close">×</button></div>
                <div class="tab" data-resource-kind="diff"><button class="tab__activate" aria-label="Diff src/Changes.tsx" title="Diff src/Changes.tsx"><svg width="16" height="16" viewBox="0 0 16 16"></svg><span class="tab__path"><span class="tab__dir">src/</span><span class="tab__basename">Changes.tsx</span></span></button><button class="tab__close">×</button></div>
                <div class="tab tab--active" data-resource-kind="browser"><button class="tab__activate" aria-label="Browser Web browser - Wikipedia" title="Browser Web browser - Wikipedia"><svg width="16" height="16" viewBox="0 0 16 16"></svg><span class="tab__path"><span class="tab__dir"></span><span class="tab__basename">Web browser - Wikipedia</span></span></button><button class="tab__close">×</button></div>
                <div class="tab" data-resource-kind="preview"><button class="tab__activate" aria-label="Preview" title="Preview"><svg width="16" height="16" viewBox="0 0 16 16"></svg><span class="tab__path"><span class="tab__basename">Preview</span></span></button></div>
              </div>
            </nav>
            <button class="agent-artifact-pane__follow"><span>Following latest</span></button>
            <button class="agent-artifact-pane__collapse">+</button>
            <button class="agent-artifact-pane__collapse">↗</button>
            <button class="agent-artifact-pane__collapse">▣</button>
          </header>
        </aside>
      </div>
    </div>
  `,
    { waitUntil: "domcontentloaded" },
  );
  await page.addStyleTag({ path: "src/styles/global.css" });
  await page.addStyleTag({ path: "src/styles/workbench-chrome.css" });
  await page.addStyleTag({ path: "src/workbench/workbench-window-mode.css" });
  await page.addStyleTag({ path: "src/styles/agent-right-pane.css" });

  const pane = page.locator(".agent-artifact-pane");
  const strip = page.locator(".tab-strip__scroll");
  const follow = page.locator(".agent-artifact-pane__follow");
  const title = page.locator(
    '.tab[data-resource-kind="browser"] .tab__basename',
  );

  await expect(pane).toHaveCSS("width", "560px");
  await expect(follow).toHaveCSS("width", "36px");
  await expect(follow.locator("span")).toBeHidden();
  await expect(title).toHaveCSS("text-overflow", "ellipsis");
  await expect(
    page.locator('.tab[data-resource-kind="file"] .tab__close'),
  ).toBeHidden();
  await expect(page.locator(".tab--active .tab__close")).toBeVisible();
  const geometry = await strip.evaluate((element) => {
    const tabBoxes = [...element.querySelectorAll(".tab")].map((tab) =>
      tab.getBoundingClientRect(),
    );
    const stripBox = element.getBoundingClientRect();
    return {
      stripWidth: element.clientWidth,
      contentWidth: element.scrollWidth,
      lastTabRight: tabBoxes.at(-1)?.right ?? 0,
      stripRight: stripBox.right,
    };
  });
  expect(geometry.contentWidth).toBeLessThanOrEqual(geometry.stripWidth + 1);
  expect(geometry.lastTabRight).toBeLessThanOrEqual(geometry.stripRight + 1);
  const titleWidths = await strip.evaluate((element) => {
    const canvas = document.createElement("canvas");
    const context = canvas.getContext("2d");
    if (!context) return [];
    return [...element.querySelectorAll<HTMLElement>(".tab__basename")].map(
      (title) => {
        const style = getComputedStyle(title);
        context.font = style.font;
        return {
          kind: title.closest<HTMLElement>(".tab")?.dataset.resourceKind,
          actual: title.clientWidth,
          sixCharacters: context.measureText(
            `${title.textContent?.slice(0, 6) ?? ""}…`,
          ).width,
        };
      },
    );
  });
  expect(titleWidths).toHaveLength(4);
  for (const width of titleWidths)
    expect(width.actual, width.kind).toBeGreaterThanOrEqual(
      width.sixCharacters,
    );
  expect(
    await title.evaluate(
      (element) => element.scrollWidth > element.clientWidth,
    ),
  ).toBe(true);
  await page.locator('.tab[data-resource-kind="file"]').hover();
  await expect(
    page.locator('.tab[data-resource-kind="file"] .tab__close'),
  ).toBeVisible();
  const crowded = await strip.evaluate((element) => {
    const browserTab = element.querySelector<HTMLElement>(
      '.tab[data-resource-kind="browser"]',
    );
    if (!browserTab) return false;
    for (let index = 0; index < 3; index += 1)
      element.append(browserTab.cloneNode(true));
    return element.scrollWidth > element.clientWidth;
  });
  expect(crowded).toBe(true);
});
