import { expect, test, type Page } from "@playwright/test";

import {
  openAgentScenario,
  openAgentSnapshotDiff,
} from "./resourceVisualHelpers";
import {
  openScenario,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";

// WebKit on macOS maps a bare `monospace` to Courier, so every monospaced
// run of text must fall back through the app's stack, which names a real
// macOS face before the generic.

async function monospaceFamilies(page: Page) {
  return page.evaluate(() => {
    // Resolve the token through a probe so the expectation names a role.
    const probe = document.createElement("span");
    probe.style.fontFamily = "var(--font-family-mono)";
    document.body.append(probe);
    const stack = getComputedStyle(probe).fontFamily;
    probe.remove();
    const families = new Map<string, string>();
    for (const element of document.querySelectorAll("body *")) {
      const family = getComputedStyle(element).fontFamily;
      if (/\bmonospace\b/.test(family))
        families.set(family, element.tagName.toLowerCase());
    }
    return { stack, families: [...families] };
  });
}

async function expectAppStack(page: Page) {
  const { stack, families } = await monospaceFamilies(page);
  expect(stack).toMatch(/ui-monospace.*Menlo.*monospace$/);
  expect(families.length).toBeGreaterThan(0);
  // A surface may put its own faces first, but it must end in the app stack.
  for (const [family, element] of families)
    expect(family.endsWith(stack), `${element}: ${family}`).toBe(true);
}

test.describe("Monospace text", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  test("chat code blocks and inline code use the app stack", async ({
    page,
  }) => {
    await openScenario(page, "agent-dual", "agent");
    await settleTranscriptAtEnd(page);
    await expect(
      page.locator(".acp-code-block pre code").first(),
    ).toBeVisible();
    await expectAppStack(page);
  });

  test("Diff lines use the app stack", async ({ page }) => {
    await openAgentScenario(page, "diff");
    await openAgentSnapshotDiff(page);
    await expect(page.locator(".diff-line").first()).toBeVisible();
    await expectAppStack(page);
  });

  test("the Editor document uses the app stack", async ({ page }) => {
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-content")).toBeVisible();
    await expectAppStack(page);
  });
});
