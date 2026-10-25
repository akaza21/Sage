import { describe, expect, it } from "vitest";
import {
  annotationBadgeOrigin,
  annotationFieldBox,
} from "./annotationGeometry";

const line = (left: number, right: number, top: number) => ({
  left,
  right,
  top,
  bottom: top + 21,
});

function intersects(
  a: { left: number; top: number; width: number; height: number },
  b: { left: number; top: number; width: number; height: number },
) {
  return (
    a.left < b.left + b.width &&
    a.left + a.width > b.left &&
    a.top < b.top + b.height &&
    a.top + a.height > b.top
  );
}

describe("annotation comment field (Codex s2/f0095)", () => {
  it("keeps a short selection's badge visible beside the field at 1888", () => {
    const selected = line(826, 938, 310);
    const scrollport = { left: 391, top: 92, right: 1888, bottom: 920 };
    const field = annotationFieldBox(
      { first: selected, last: selected },
      660,
      scrollport,
    );
    const badge = annotationBadgeOrigin(selected);
    expect(field.width).toBe(270);
    expect(
      intersects({ ...field, height: 44 }, { ...badge, width: 24, height: 24 }),
    ).toBe(false);
    expect(field.top).toBe(310 - 44 - 6);
    expect(field.left).toBeGreaterThanOrEqual(scrollport.left);
  });

  it("sizes to the column and stays inside the scrollport at 480", () => {
    const selected = line(300, 420, 300);
    const scrollport = { left: 262, top: 92, right: 480, bottom: 700 };
    const field = annotationFieldBox(
      { first: selected, last: selected },
      186,
      scrollport,
    );
    expect(field.width).toBe(186);
    expect(field.left).toBeGreaterThanOrEqual(scrollport.left);
    expect(field.left + field.width).toBeLessThanOrEqual(scrollport.right);
    // No room left of the badge here, so the field drops below the line.
    expect(field.top).toBe(300 + 21 + 6);
    expect(
      intersects(
        { ...field, height: 44 },
        { ...annotationBadgeOrigin(selected), width: 24, height: 24 },
      ),
    ).toBe(false);
  });

  it("never exceeds 270 px or a narrow scrollport", () => {
    const selected = line(40, 60, 200);
    const field = annotationFieldBox({ first: selected, last: selected }, 600, {
      left: 0,
      top: 0,
      right: 200,
      bottom: 400,
    });
    expect(field.width).toBe(184);
    expect(field.left).toBe(8);
  });
});
