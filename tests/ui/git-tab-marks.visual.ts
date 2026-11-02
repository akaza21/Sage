import { expect, test, type Page } from "@playwright/test";

// EP-13: a File tab's Git mark must take exactly the room the tab held for it
// while the decoration chunk loaded, so the strip never shifts. These check
// real layout with the shipped stylesheets, which DOM-only tests cannot.

const LETTERS = ["M", "T", "A", "R", "C", "U", "D", "!"] as const;
const TITLE = "SettingsPanel.tsx";

/** A File tab as TabStrip renders it: holding the slot, or with its mark. */
function fileTab(state: "pending" | "loaded", letter: string) {
  const mark =
    state === "pending"
      ? `<span>${TITLE}</span><span class="tab__git-slot" aria-hidden="true"></span>`
      : `<span class="tab__title--git git-tone--modified">${TITLE}</span><span class="sr-only">, Modified</span><span class="git-badge git-tone--modified" title="Modified" aria-hidden="true">${letter}</span>`;
  return `<div class="tab" role="presentation" data-state="${state}"><button class="tab__activate" type="button" role="tab"><svg width="16" height="16" viewBox="0 0 16 16"></svg>${mark}</button><button class="tab__close" type="button">×</button></div>`;
}

async function mountStrips(page: Page, stripWidth: number, letter: string) {
  const tabs = (state: "pending" | "loaded") =>
    `<nav class="tab-strip" style="width: ${stripWidth}px"><div class="tab-strip__scroll">${[
      0, 1, 2,
    ]
      .map(() => fileTab(state, letter))
      .join("")}</div></nav>`;
  await page.setContent(
    `<div class="workbench__tabs" data-strip="pending">${tabs("pending")}</div>
     <div class="workbench__tabs" data-strip="loaded">${tabs("loaded")}</div>`,
    { waitUntil: "domcontentloaded" },
  );
  for (const path of [
    "src/styles/global.css",
    "src/styles/workbench-chrome.css",
    "src/features/git/git-decorations.css",
  ])
    await page.addStyleTag({ path });
}

/** Each tab's box, its title's box and its last child's box, by strip. */
function geometry(page: Page, strip: "pending" | "loaded") {
  return page
    .locator(`[data-strip="${strip}"] .tab-strip__scroll`)
    .evaluate((element) =>
      [...element.querySelectorAll<HTMLElement>(".tab")].map((tab) => {
        const button = tab.querySelector<HTMLElement>(".tab__activate")!;
        const box = (node: Element) => {
          const rect = node.getBoundingClientRect();
          return {
            left: Math.round(rect.left * 100) / 100,
            width: Math.round(rect.width * 100) / 100,
          };
        };
        return {
          tab: box(tab),
          title: box(button.querySelector("span")!),
          mark: box(button.lastElementChild!),
          close: box(tab.querySelector(".tab__close")!),
        };
      }),
    );
}

for (const [label, stripWidth] of [
  ["normal", 1200],
  ["narrow", 300],
] as const) {
  test(`a File tab's Git mark lands in the room held for it (${label} strip)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 400 });
    for (const letter of LETTERS) {
      await mountStrips(page, stripWidth, letter);
      const pending = await geometry(page, "pending");
      const loaded = await geometry(page, "loaded");
      expect(loaded, `letter ${letter}`).toEqual(pending);
      // The held room is real, not zero width.
      expect(pending[0].mark.width).toBeGreaterThan(4);
    }
  });
}

test("Agent artifact tabs still reveal Close on hover, focus and the active tab", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 400 });
  await page.setContent(
    `<div class="workbench--agent"><aside class="agent-artifact-pane" style="width: 560px"><nav class="tab-strip"><div class="tab-strip__scroll">
       <div class="tab" data-id="idle"><button class="tab__activate" type="button"><span>a.ts</span></button><button class="tab__close" type="button">×</button></div>
       <div class="tab" data-id="hover"><button class="tab__activate" type="button"><span>b.ts</span></button><button class="tab__close" type="button">×</button></div>
       <div class="tab tab--active" data-id="active"><button class="tab__activate" type="button"><span>c.ts</span></button><button class="tab__close" type="button">×</button></div>
     </div></nav></aside></div>`,
    { waitUntil: "domcontentloaded" },
  );
  for (const path of [
    "src/styles/global.css",
    "src/styles/workbench-chrome.css",
    "src/workbench/workbench-window-mode.css",
    "src/styles/agent-right-pane.css",
    "src/features/git/git-decorations.css",
  ])
    await page.addStyleTag({ path });

  const close = (id: string) => page.locator(`[data-id="${id}"] .tab__close`);
  await expect(close("idle")).toBeHidden();
  await expect(close("active")).toBeVisible();
  await page.locator('[data-id="hover"]').hover();
  await expect(close("hover")).toBeVisible();
  await page.mouse.move(0, 300);
  await expect(close("hover")).toBeHidden();
  await page.locator('[data-id="idle"] .tab__activate').focus();
  await expect(close("idle")).toBeVisible();
});
