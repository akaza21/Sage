import { readFileSync } from "node:fs";
import { expect, test, type Page } from "./shared/test";

const style = readFileSync(
  "src-tauri/src/window_frame/window-controls.css",
  "utf8",
);
const adapter = readFileSync(
  "src-tauri/src/window_frame/window-controls.js",
  "utf8",
).replace('/* SAGE_WINDOW_FRAME_STYLE */ ""', JSON.stringify(style));

type CaptionProbe = {
  layout: { height: number; palette: { background: number } } | null;
  mutationRecords: number;
};
type CaptionWindow = Window & { captionProbe: CaptionProbe };

async function settled(page: Page) {
  // Let real MutationObserver, ResizeObserver, and rAF delivery drain.
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript({
    content: `
      window.captionProbe = { layout: null, mutationRecords: 0 };
      // Native IPC is the external boundary. Retain its output, not call counts.
      window.__TAURI_INTERNALS__ = {
        invoke: async (command, { layout }) => {
          if (command !== "set_window_caption_controls") throw Error(command);
          window.captionProbe.layout = layout;
        },
      };
      // Use native observer delivery to measure caption work, scoped to the
      // injected adapter so React and other application observers are excluded.
      ((MutationObserver) => { ${adapter} })(class extends window.MutationObserver {
        constructor(callback) {
          super((records, observer) => {
            window.captionProbe.mutationRecords += records.length;
            callback(records, observer);
          });
        }
      });
    `,
  });
  await page.goto("/tests/ui/fixtures/window-frame.html");
  await expect(page.locator(".title-bar")).toHaveAttribute(
    "data-sage-window-frame",
    "",
  );
  await expect
    .poll(() =>
      page.evaluate(() => (window as CaptionWindow).captionProbe.layout),
    )
    .not.toBeNull();
  await settled(page);
});

test("transcript and editor rendering deliver no caption mutation work", async ({
  page,
}) => {
  const records = await page.evaluate(async () => {
    const probe = (window as CaptionWindow).captionProbe;
    probe.mutationRecords = 0;
    const transcript = document.querySelector('[aria-label="Transcript"]')!;
    const editor = document.querySelector('[aria-label="Editor"]')!;
    for (let i = 0; i < 100; i++) {
      transcript.append(document.createTextNode("streamed token "));
      editor.replaceChildren(document.createTextNode(`edited line ${i}`));
      await Promise.resolve();
    }
    return probe.mutationRecords;
  });
  expect(records).toBe(0);
});

test("caption follows title-bar removal, replacement, geometry, and palette", async ({
  page,
}) => {
  const layout = () =>
    page.evaluate(() => (window as CaptionWindow).captionProbe.layout);
  const original = await page.locator(".title-bar").elementHandle();
  await page.getByRole("button", { name: "Toggle title bar" }).click();
  await expect.poll(layout).toBeNull();
  expect(await original!.getAttribute("data-sage-window-frame")).toBeNull();
  expect(
    await original!.evaluate((header) =>
      header.style.getPropertyValue("--sage-window-controls-width"),
    ),
  ).toBe("");
  await page.getByRole("button", { name: "Toggle title bar" }).click();
  await expect.poll(layout).not.toBeNull();
  await page.getByRole("button", { name: "Replace title bar" }).click();
  await expect(page.locator(".title-bar")).toHaveAttribute(
    "data-sage-window-frame",
    "",
  );
  await page.locator(".title-bar").evaluate((header) => {
    header.style.height = "64px";
    header.style.backgroundColor = "rgb(17, 34, 51)";
  });
  await expect.poll(async () => (await layout())?.height).toBe(64);
  await expect
    .poll(async () => (await layout())?.palette.background)
    .toBe(0x332211);
  await page.getByRole("button", { name: "Toggle title bar" }).click();
  await expect.poll(layout).toBeNull();
  await settled(page);
  const records = await page.evaluate(async () => {
    const probe = (window as CaptionWindow).captionProbe;
    probe.mutationRecords = 0;
    document.body.style.backgroundColor = "rgb(51, 34, 17)";
    await Promise.resolve();
    return probe.mutationRecords;
  });
  expect(records).toBe(0);
});
