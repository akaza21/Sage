export const UI_FONT_SIZES = [12, 13, 14, 16] as const;
export const UI_DENSITIES = ["comfortable", "compact"] as const;

export type UiFontSize = (typeof UI_FONT_SIZES)[number];
export type UiDensity = (typeof UI_DENSITIES)[number];

export interface UiAppearance {
  fontSize: UiFontSize;
  density: UiDensity;
}

export const DEFAULT_UI_APPEARANCE: UiAppearance = {
  fontSize: 14,
  density: "comfortable",
};

const DENSITY_COPY: Record<UiDensity, { label: string; description: string }> =
  {
    comfortable: {
      label: "Comfortable",
      description: "Standard row heights and spacing across the workbench.",
    },
    compact: {
      label: "Compact",
      description: "Tighter rows and spacing so more fits on screen.",
    },
  };

function isUiFontSize(value: unknown): value is UiFontSize {
  return UI_FONT_SIZES.some((size) => size === value);
}

function isUiDensity(value: unknown): value is UiDensity {
  return UI_DENSITIES.some((density) => density === value);
}

export function admitUiAppearance(input: unknown): UiAppearance {
  const source: Record<string, unknown> =
    typeof input === "object" && input !== null
      ? (input as Record<string, unknown>)
      : {};
  const fontSize = source.fontSize;
  const density = source.density;
  return {
    fontSize: isUiFontSize(fontSize)
      ? fontSize
      : DEFAULT_UI_APPEARANCE.fontSize,
    density: isUiDensity(density) ? density : DEFAULT_UI_APPEARANCE.density,
  };
}

export function uiDensityCopy(density: UiDensity) {
  return DENSITY_COPY[density];
}
