import { readdirSync, readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

type Rule = {
  id: string;
  ref: string;
  selector: string;
  measure:
    | "height"
    | "width"
    | "background-color"
    | "border-radius"
    | "text-align"
    | "placeholder"
    | "left-of"
    | "below"
    | "right-inset";
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

const RULES = "docs/work-items/BROWSER-FIRST-CLASS/ui-rules";
const ruleSets: [string, RuleSet][] = readdirSync(RULES)
  .filter((name) => name.endsWith(".json"))
  .map((name) => [name, JSON.parse(readFileSync(`${RULES}/${name}`, "utf8"))]);

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

for (const [name, ruleSet] of ruleSets) {
  test.describe(name, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize(ruleSet.viewport);
      await page.goto(ruleSet.fixture);
    });
    for (const rule of ruleSet.rules) {
      test(`${rule.id} matches Codex ${rule.ref}`, async ({ page }) => {
        await check(page, rule);
      });
    }
  });
}

async function check(page: Page, rule: Rule) {
  switch (rule.measure) {
    case "height":
    case "width": {
      const measured = (await box(page, rule.selector))[rule.measure];
      expect(Math.abs(measured - Number(rule.expected))).toBeLessThanOrEqual(
        rule.tolerancePx ?? 0,
      );
      break;
    }
    case "background-color":
    case "border-radius":
    case "text-align":
      expect(await style(page, rule.selector, rule.measure)).toBe(
        rule.expected,
      );
      break;
    case "placeholder":
      await expect(page.locator(rule.selector).first()).toHaveAttribute(
        "placeholder",
        String(rule.expected),
      );
      break;
    case "left-of": {
      const left = await box(page, rule.selector);
      const right = await box(page, rule.other!);
      expect(left.x + left.width).toBeLessThanOrEqual(right.x);
      break;
    }
    case "below": {
      const lower = await box(page, rule.selector);
      const upper = await box(page, rule.other!);
      expect(lower.y).toBeGreaterThanOrEqual(upper.y + upper.height);
      break;
    }
    case "right-inset": {
      const element = await box(page, rule.selector);
      const container = await box(page, rule.within!);
      const inset = container.x + container.width - (element.x + element.width);
      expect(Math.abs(inset - Number(rule.expected))).toBeLessThanOrEqual(
        rule.tolerancePx ?? 0,
      );
      break;
    }
    default:
      throw new Error(`rule ${rule.id} uses unknown measure ${rule.measure}`);
  }
}
