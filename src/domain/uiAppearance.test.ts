import { describe, expect, it } from "vitest";

import {
  DEFAULT_UI_APPEARANCE,
  UI_DENSITIES,
  UI_FONT_SIZES,
  admitUiAppearance,
  uiDensityCopy,
} from "./uiAppearance";

describe("ui appearance", () => {
  it("offers the bounded font size and density ramps", () => {
    expect([...UI_FONT_SIZES]).toEqual([12, 13, 14, 16]);
    expect([...UI_DENSITIES]).toEqual(["comfortable", "compact"]);
  });

  it("defaults to the reference body size at comfortable density", () => {
    expect(DEFAULT_UI_APPEARANCE).toEqual({
      fontSize: 14,
      density: "comfortable",
    });
  });

  it("degrades unknown values to the default instead of throwing", () => {
    expect(admitUiAppearance({ fontSize: 11, density: "roomy" })).toEqual(
      DEFAULT_UI_APPEARANCE,
    );
    expect(admitUiAppearance(null)).toEqual(DEFAULT_UI_APPEARANCE);
    expect(admitUiAppearance(undefined)).toEqual(DEFAULT_UI_APPEARANCE);
  });

  it("keeps admitted values", () => {
    expect(admitUiAppearance({ fontSize: 16, density: "compact" })).toEqual({
      fontSize: 16,
      density: "compact",
    });
  });

  it("names each density for the Appearance page", () => {
    expect(uiDensityCopy("comfortable").label).toBe("Comfortable");
    expect(uiDensityCopy("compact").label).toBe("Compact");
    expect(uiDensityCopy("compact").description).toMatch(/row/i);
  });
});
