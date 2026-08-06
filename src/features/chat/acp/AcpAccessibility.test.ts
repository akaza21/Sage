import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

const globalStyles = readFileSync(resolve("src/styles/global.css"), "utf8");
const acpStyles = readFileSync(
  resolve("src/features/chat/acp/acp-chat.css"),
  "utf8",
);

let globalStyle: HTMLStyleElement;
let acpStyle: HTMLStyleElement;

beforeEach(() => {
  globalStyle = document.createElement("style");
  globalStyle.textContent = globalStyles;
  acpStyle = document.createElement("style");
  acpStyle.textContent = acpStyles;
  document.head.append(globalStyle, acpStyle);
});

afterEach(() => {
  globalStyle.remove();
  acpStyle.remove();
});

describe("ACP accessibility rendering contract", () => {
  it("does not force the page wider than a 200 percent zoom viewport", () => {
    const pageRule = [...(globalStyle.sheet?.cssRules ?? [])].find(
      (rule): rule is CSSStyleRule =>
        rule instanceof CSSStyleRule &&
        rule.selectorText.replaceAll(/\s+/g, " ") === "html, body, #root",
    );

    expect(pageRule).toBeDefined();
    expect(pageRule?.style.minWidth).toBe("0px");
  });

  it("defines forced-color and zero-duration ACP rendering overrides", () => {
    const mediaRules = [...(acpStyle.sheet?.cssRules ?? [])].filter(
      (rule): rule is CSSMediaRule => rule instanceof CSSMediaRule,
    );
    const reducedMotion = mediaRules.find(
      (rule) => rule.conditionText === "(prefers-reduced-motion: reduce)",
    );
    const motionRule = [...(reducedMotion?.cssRules ?? [])].find(
      (rule): rule is CSSStyleRule =>
        rule instanceof CSSStyleRule &&
        rule.selectorText === ".acp-chat-projection *",
    );
    expect(motionRule?.style.animationDuration).toBe("0s");
    expect(motionRule?.style.transitionDuration).toBe("0s");
    expect(motionRule?.style.scrollBehavior).toBe("auto");

    const forcedColors = mediaRules.find(
      (rule) => rule.conditionText === "(forced-colors: active)",
    );
    const cardRule = [...(forcedColors?.cssRules ?? [])].find(
      (rule): rule is CSSStyleRule =>
        rule instanceof CSSStyleRule &&
        rule.selectorText.includes(".acp-permission"),
    );
    expect(cardRule?.style.borderColor.toLowerCase()).toBe("canvastext");
  });
});
