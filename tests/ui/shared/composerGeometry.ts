import { expect, type Locator } from "@playwright/test";

type SettleWindow = Window & {
  settleComposerRow?: (row: Element) => Promise<void>;
};

/**
 * Installs `window.settleComposerRow(row)`, which resolves once the row's
 * overflow fit has run for its current geometry.
 *
 * The fit refits on the frame after a ResizeObserver delivery, and under CPU
 * load a delivery can lag several frames behind layout, so a run of
 * unchanged frames does not prove the row is settled. This observes the row
 * and its controls as the fit does, and resolves on the first frame after
 * every observed element's current size has been delivered.
 */
export function installComposerRowSettle() {
  const page = window as SettleWindow;
  page.settleComposerRow ??= (row) =>
    new Promise((resolve) => {
      const delivered = new Map<Element, string>();
      const size = (width: number, height: number) => `${width}x${height}`;
      const targets = () => [row, ...row.children];
      let check = 0;
      const observer = new ResizeObserver((entries) => {
        for (const entry of entries) {
          const [box] = entry.borderBoxSize;
          delivered.set(entry.target, size(box.inlineSize, box.blockSize));
        }
        // The fit runs on the next frame; check after it.
        cancelAnimationFrame(check);
        check = requestAnimationFrame(() => {
          const pending = targets().filter((target) => {
            observer.observe(target);
            const box = target.getBoundingClientRect();
            return delivered.get(target) !== size(box.width, box.height);
          });
          if (pending.length > 0) return;
          observer.disconnect();
          resolve();
        });
      });
      for (const target of targets()) observer.observe(target);
    });
}

/**
 * The composer toolbar's compaction steps once its fit has run for the
 * current layout. An open popover holds the steps, so a test settles the row
 * before it reads where a control is or opens a menu.
 */
export async function settledComposerFit(composer: Locator) {
  await composer.page().evaluate(installComposerRowSettle);
  const fit = await composer
    .locator(".composer__controls")
    .evaluate(async (row) => {
      await (window as SettleWindow).settleComposerRow!(row);
      return row.getAttribute("data-fit") ?? "";
    });
  return fit.split(" ").filter(Boolean);
}

export async function expectCompactComposerTargets(composer: Locator) {
  const targets = await composer
    .locator(
      [
        ".composer__plus",
        ".composer__access-label",
        ".composer__harness button.searchable-picker__trigger",
        ".composer__model-trigger",
        ".composer__effort-picker",
        ".project-scope button.searchable-picker__trigger",
        ".composer__mic",
        ".send-button",
      ].join(","),
    )
    .evaluateAll((elements) =>
      elements
        .map((element) => {
          const { x, y, width, height } = element.getBoundingClientRect();
          return { x, y, width, height };
        })
        .filter((box) => box.width > 0 && box.height > 0),
    );
  expect(targets.length).toBeGreaterThanOrEqual(5);
  const ordered = targets.sort((left, right) => left.x - right.x);
  const bounds = (await composer.boundingBox())!;
  for (const [index, box] of ordered.entries()) {
    expect(box.width).toBeGreaterThanOrEqual(24);
    expect(box.height).toBeGreaterThanOrEqual(24);
    expect(box.x).toBeGreaterThanOrEqual(bounds.x);
    expect(box.x + box.width).toBeLessThanOrEqual(bounds.x + bounds.width);
    expect(
      Math.abs(box.y + box.height / 2 - (ordered[0].y + ordered[0].height / 2)),
    ).toBeLessThanOrEqual(1);
    if (index > 0)
      expect(box.x).toBeGreaterThanOrEqual(
        ordered[index - 1].x + ordered[index - 1].width,
      );
  }
}
