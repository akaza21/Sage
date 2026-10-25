import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";

type Rule = {
  id: string;
  ref: string;
  selector: string;
  measure:
    "height" | "background-color" | "text-align" | "left-of" | "right-inset";
  expected?: number | string;
  tolerancePx?: number;
  other?: string;
  within?: string;
};

type RuleSet = {
  fixture: string;
  viewport: { width: number; height: number };
  rules: Rule[];
};

const ruleSet: RuleSet = JSON.parse(
  readFileSync(
    "docs/work-items/BROWSER-FIRST-CLASS/ui-rules/browser-chrome-v1.json",
    "utf8",
  ),
);

async function box(page: Page, selector: string) {
  const element = page.locator(selector).first();
  await expect(element).toBeVisible();
  const value = await element.boundingBox();
  if (!value) throw new Error(`${selector} has no layout box`);
  return value;
}

async function style(page: Page, selector: string, property: string) {
  return page
    .locator(selector)
    .first()
    .evaluate(
      (element, name) => getComputedStyle(element).getPropertyValue(name),
      property,
    );
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize(ruleSet.viewport);
  await page.goto(ruleSet.fixture);
});

for (const rule of ruleSet.rules) {
  test(`${rule.id} matches Codex ${rule.ref}`, async ({ page }) => {
    switch (rule.measure) {
      case "height": {
        const { height } = await box(page, rule.selector);
        expect(Math.abs(height - Number(rule.expected))).toBeLessThanOrEqual(
          rule.tolerancePx ?? 0,
        );
        break;
      }
      case "background-color":
      case "text-align":
        expect(await style(page, rule.selector, rule.measure)).toBe(
          rule.expected,
        );
        break;
      case "left-of": {
        const left = await box(page, rule.selector);
        const right = await box(page, rule.other!);
        expect(left.x + left.width).toBeLessThanOrEqual(right.x);
        break;
      }
      case "right-inset": {
        const element = await box(page, rule.selector);
        const container = await box(page, rule.within!);
        const inset =
          container.x + container.width - (element.x + element.width);
        expect(Math.abs(inset - Number(rule.expected))).toBeLessThanOrEqual(
          rule.tolerancePx ?? 0,
        );
        break;
      }
    }
  });
}
