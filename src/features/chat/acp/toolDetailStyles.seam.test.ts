import { readFileSync } from "node:fs";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

const source = readFileSync(
  path.resolve(import.meta.dirname, "acp-activity-details.css"),
  "utf8",
);

afterEach(() => {
  document.head.innerHTML = "";
});

function stylesInMedia(condition: string, selector: string) {
  const style = document.createElement("style");
  style.textContent = source;
  document.head.append(style);
  const media = [...style.sheet!.cssRules].find(
    (rule) => rule instanceof CSSMediaRule && rule.conditionText === condition,
  ) as CSSMediaRule;
  return [...media.cssRules].filter(
    (rule): rule is CSSStyleRule =>
      rule instanceof CSSStyleRule &&
      rule.selectorText.split(",").some((part) => part.trim() === selector),
  );
}

describe("tool detail style contract", () => {
  it("declares a static running icon and readable label under reduced motion", () => {
    const condition = "(prefers-reduced-motion: reduce)";
    expect(
      stylesInMedia(condition, ".acp-tool-running > svg")[0].style.animation,
    ).toBe("none");
    const labels = stylesInMedia(condition, ".acp-live-shimmer");
    expect(labels[0].style.animation).toBe("none");
    expect(labels[1].style.background).toBe("none");
    expect(labels[1].style.color).toBe("var(--color-text-muted)");
  });

  it("declares system text colors for the running indicator and label under forced colors", () => {
    for (const selector of [".acp-tool-running", ".acp-live-shimmer"]) {
      const rule = stylesInMedia("(forced-colors: active)", selector)[0];
      expect(rule.style.color).toBe("canvastext");
      expect(rule.style.background).toBe("none");
    }
  });
});
