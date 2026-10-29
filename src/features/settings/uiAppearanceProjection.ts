import type { UiAppearance } from "../../domain/uiAppearance";

export function projectUiAppearance(appearance: UiAppearance) {
  const style = document.documentElement.style;
  style.setProperty("--font-size-md", `${appearance.fontSize}px`);
  const height = appearance.density === "compact" ? "28px" : "32px";
  style.setProperty("--row-height-default", height);
  style.setProperty("--control-height-default", height);
}
