import { expect, test, type Page } from "@playwright/test";
import { build } from "vite";

let script: string;
let styles: string;
test.beforeAll(async () => {
  const result = await build({
    configFile: false,
    publicDir: false,
    logLevel: "silent",
    define: { "process.env.NODE_ENV": JSON.stringify("production") },
    build: {
      write: false,
      lib: {
        entry: "tests/ui/gap-b-fixture.tsx",
        formats: ["iife"],
        name: "GapB",
      },
    },
  });
  const output = (Array.isArray(result) ? result : [result]).flatMap(
    (bundle) => {
      if (!("output" in bundle)) throw new Error("Expected fixture output");
      return bundle.output;
    },
  );
  script = output
    .filter((entry) => entry.type === "chunk")
    .map((entry) => entry.code)
    .join("\n");
  styles = output
    .filter(
      (entry) => entry.type === "asset" && entry.fileName.endsWith(".css"),
    )
    .map((entry) => String(entry.source))
    .join("\n");
});
test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 480, height: 900 });
  await page.setContent(`<style>${styles}</style><main id="root"></main>`);
  await page.addScriptTag({ content: script });
});
async function render(page: Page, surface: string) {
  await page.evaluate(
    (name) =>
      (window as typeof window & { renderGapB(name: string): void }).renderGapB(
        name,
      ),
    surface,
  );
}

test("composer composition preserves text, focus is visible, and image/actions fit 480px", async ({
  page,
}) => {
  await render(page, "composer");
  const draft = page.getByRole("textbox", { name: "Message agent" });
  await page.keyboard.press("Tab");
  await expect(draft).toBeFocused();
  expect(
    await page
      .locator(".composer")
      .evaluate((node) => getComputedStyle(node).boxShadow),
  ).not.toBe("none");
  await draft.fill("日本語");
  await draft.dispatchEvent("keydown", { key: "Enter", isComposing: true });
  await expect(page.getByLabel("Submitted text")).toBeEmpty();
  await draft.press("Enter");
  await expect(page.getByLabel("Submitted text")).toHaveText("日本語");
  await expect(
    page.getByRole("button", { name: "Send message" }),
  ).toBeVisible();
  await expect(page.locator(".composer img")).toBeVisible();
  expect(
    await page
      .locator(".composer")
      .evaluate((node) => node.scrollWidth <= node.clientWidth),
  ).toBe(true);
});

test("keyboard search reaches a model beyond 200 and reports no match", async ({
  page,
}) => {
  await render(page, "models");
  await expect(page.getByRole("option")).toHaveCount(200);
  const search = page.getByRole("searchbox", { name: "Search models" });
  await search.fill("Model 203");
  await search.press("ArrowDown");
  await expect(page.getByRole("option", { name: "Model 203" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByLabel("Selected model")).toHaveText("model-203");
  await search.fill("missing");
  await expect(page.getByRole("option")).toHaveCount(0);
  await expect(page.locator(".menu-empty[role=status]")).toHaveText(
    "No models match.",
  );
});

test("code scroll and inset focus stay within the card, and Wrap contains source", async ({
  page,
}) => {
  await render(page, "code");
  const pre = page.locator("pre");
  await pre.focus();
  const geometry = await pre.evaluate((node) => ({
    width: node.clientWidth,
    scroll: node.scrollWidth,
    outline: getComputedStyle(node).outlineOffset,
  }));
  expect(geometry.scroll).toBeGreaterThan(geometry.width);
  expect(Number.parseFloat(geometry.outline)).toBeLessThan(0);
  await pre.press("ArrowRight");
  await expect
    .poll(() => pre.evaluate((node) => node.scrollLeft))
    .toBeGreaterThan(0);
  await page.getByRole("button", { name: "Wrap code" }).click();
  expect(
    await pre.evaluate((node) => node.scrollWidth <= node.clientWidth),
  ).toBe(true);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});

test("user hover and keyboard actions leave bubble geometry stable; date has no hairlines", async ({
  page,
}) => {
  await render(page, "user");
  const bubble = page.locator(".acp-message--user");
  const before = await bubble.boundingBox();
  const copy = page.getByRole("button", { name: "Copy message" });
  await copy.focus();
  await expect(copy).toBeVisible();
  await bubble.hover();
  expect(await bubble.boundingBox()).toEqual(before);
  expect(
    await page
      .locator(".chat-transcript-day")
      .evaluate((node) => getComputedStyle(node, "::before").content),
  ).toBe("none");
});

test("home project stays inside its headline and the edited file opens its saved diff", async ({
  page,
}) => {
  await render(page, "home");
  const heading = page.getByRole("heading", {
    name: /What should we work on in A very long/,
  });
  await expect(heading.locator(".project-scope")).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await render(page, "files");
  await expect(page.getByText("Edited a.ts", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "View changes" }).click();
  await expect(page.getByLabel("Opened resource")).toHaveText("diff:src/a.ts");
});
