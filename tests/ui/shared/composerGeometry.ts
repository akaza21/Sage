import { expect, type Locator } from "@playwright/test";

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
