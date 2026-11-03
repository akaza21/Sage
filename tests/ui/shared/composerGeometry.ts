import { expect, type Locator } from "@playwright/test";

/**
 * The composer toolbar's compaction steps once they stop changing. A resize
 * refits the row on the next frame, and an open popover holds its steps, so a
 * test settles the row before it reads where a control is or opens a menu.
 */
export async function settledComposerFit(composer: Locator) {
  const fit = await composer
    .locator(".composer__controls")
    .evaluate(async (row) => {
      const frame = () => new Promise(requestAnimationFrame);
      let current = row.getAttribute("data-fit");
      for (let stable = 0; stable < 3;) {
        await frame();
        const next = row.getAttribute("data-fit");
        stable = next === current ? stable + 1 : 0;
        current = next;
      }
      return current ?? "";
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
