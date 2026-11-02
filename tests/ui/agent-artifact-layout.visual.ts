import { expect, test } from "@playwright/test";

test("selected overflow tab stays fully visible on selection, keyboard navigation and resize", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/artifact-tabs.html");
  await page.getByRole("button", { name: "Select last resource" }).click();
  const selected = page.getByRole("tab", { selected: true });
  const visibleTab = async () => {
    const geometry = await selected.evaluate((tab) => {
      const row = tab.closest(".tab")!.getBoundingClientRect();
      const strip = tab.closest(".tab-strip__scroll")!.getBoundingClientRect();
      return {
        left: row.left,
        right: row.right,
        stripLeft: strip.left,
        stripRight: strip.right,
      };
    });
    expect(geometry.left).toBeGreaterThanOrEqual(geometry.stripLeft - 1);
    expect(geometry.right).toBeLessThanOrEqual(geometry.stripRight + 1);
  };
  await expect(selected).toHaveAccessibleName("Browser Browser page 8");
  await visibleTab();
  await selected.focus();
  await selected.press("Home");
  await expect(selected).toHaveAccessibleName("Browser Browser page 1");
  await visibleTab();
  await selected.press("ArrowLeft");
  await expect(selected).toHaveAccessibleName("Browser Browser page 8");
  await page.setViewportSize({ width: 400, height: 700 });
  await expect
    .poll(async () =>
      selected.evaluate((tab) => tab.closest(".tab-strip__scroll")!.scrollLeft),
    )
    .toBeGreaterThan(0);
  await visibleTab();
  await expect(
    page.getByRole("button", { name: "New browser tab" }),
  ).toBeDisabled();
});

test("Escape reverts the browser address without collapsing the artifact pane", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/browser-address.html?artifact-focus");
  const address = page.getByRole("textbox", { name: "Address or search" });
  await address.click();
  await address.fill("draft.example");
  await address.press("Escape");
  await expect(page.getByLabel("Agent artifacts")).toBeVisible();
  await address.focus();
  await expect(address).toHaveValue(
    "https://en.wikipedia.org/wiki/Web_browser",
  );
});

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

test("loaded browser address remains legible in a 560 px right-pane toolbar", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1920, height: 900 });
  await page.goto("/tests/ui/fixtures/browser-address.html");

  const toolbar = page.locator(".browser-surface__toolbar");
  const address = page.getByRole("textbox", { name: "Address or search" });
  await expect(toolbar).toHaveCSS("height", "44px");
  await expect(address).toHaveCSS("height", "32px");
  await expect(address).toHaveValue("en.wikipedia.org");
  // Codex ref 59a: the Annotate pill sits between reload and the address,
  // so the field starts after it and its text is centred inside the field.
  const geometry = await address.evaluate((element) => ({
    width: element.getBoundingClientRect().width,
    textAlign: getComputedStyle(element).textAlign,
  }));
  expect(geometry.width).toBeGreaterThanOrEqual(280);
  expect(geometry.textAlign).toBe("center");
  const annotate = page.getByRole("button", { name: "Annotate" });
  await expect(annotate).toHaveCSS("width", "28px");

  await address.click();
  await expect(address).toHaveValue(
    "https://en.wikipedia.org/wiki/Web_browser",
  );
  const selection = await address.evaluate((element: HTMLInputElement) => ({
    start: element.selectionStart,
    end: element.selectionEnd,
  }));
  expect(selection).toEqual({
    start: 0,
    end: "https://en.wikipedia.org/wiki/Web_browser".length,
  });
});
