import { expect, test } from "@playwright/test";

declare global {
  interface Window {
    chatPerf: {
      mount: (mode: "cap" | "stress") => {
        mountMs: number;
        commitCount: number;
        commitDurationMs: number;
        domNodes: number;
        renderedTurns: number;
        heapBytes: number | null;
      };
      stream: (count: number) => {
        durations: number[];
        commitCount: number;
        domNodes: number;
        heapBytes: number | null;
      };
      scrollToTop: () => number;
      scrollToMiddle: () => void;
      visibleAnchor: () => { id: string; top: number } | null;
      prepend: (count: number) => void;
      distanceFromEnd: () => number;
    };
  }
}

test("long Chat preserves the visible Turn when earlier Turns arrive", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/tests/ui/fixtures/chat-performance.html");
  await page.waitForFunction(() => Boolean(window.chatPerf));
  await page.evaluate(() => window.chatPerf.mount("cap"));
  await page.evaluate(() => window.chatPerf.scrollToMiddle());
  await page.waitForFunction(() => Boolean(window.chatPerf.visibleAnchor()));
  const before = await page.evaluate(() => window.chatPerf.visibleAnchor());
  expect(before).not.toBeNull();
  await page.evaluate(() => window.chatPerf.prepend(10));
  const after = await page.evaluate(() => window.chatPerf.visibleAnchor());
  expect(after?.id).toBe(before?.id);
  expect(Math.abs((after?.top ?? 0) - (before?.top ?? 0))).toBeLessThanOrEqual(
    4,
  );
  expect(await page.locator(".acp-turn").count()).toBeLessThanOrEqual(40);
});

test("long Chat follows streaming and Jump to latest restores the end", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/tests/ui/fixtures/chat-performance.html?follow");
  await page.waitForFunction(() => Boolean(window.chatPerf));
  await page.evaluate(() => window.chatPerf.mount("cap"));
  await page.evaluate(() => window.chatPerf.stream(3));
  await expect
    .poll(() => page.evaluate(() => window.chatPerf.distanceFromEnd()))
    .toBeLessThanOrEqual(64);
  await page.evaluate(() => window.chatPerf.scrollToTop());
  const jump = page.getByRole("button", { name: "Jump to latest" });
  await expect(jump).toBeVisible();
  await jump.evaluate((button: HTMLButtonElement) => button.click());
  await expect
    .poll(() => page.evaluate(() => window.chatPerf.distanceFromEnd()))
    .toBeLessThanOrEqual(64);
});

test("expanding a measured Turn keeps neighboring history reachable", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/tests/ui/fixtures/chat-performance.html");
  await page.waitForFunction(() => Boolean(window.chatPerf));
  await page.evaluate(() => window.chatPerf.mount("cap"));
  await page.evaluate(() => window.chatPerf.scrollToMiddle());
  await page.waitForFunction(() => Boolean(window.chatPerf.visibleAnchor()));
  const before = await page.evaluate(() => window.chatPerf.visibleAnchor());
  const disclosure = page
    .locator(`[data-turn-id="${before?.id}"] details`)
    .first();
  await expect(disclosure).toBeVisible();
  await disclosure.locator(":scope > summary").click();
  await expect(disclosure).toHaveAttribute("open", "");
  const after = await page.evaluate(() => window.chatPerf.visibleAnchor());
  expect(after?.id).toBe(before?.id);
  expect(Math.abs((after?.top ?? 0) - (before?.top ?? 0))).toBeLessThanOrEqual(
    4,
  );
  expect(await page.locator(".acp-turn").count()).toBeLessThanOrEqual(40);
  await page.evaluate(() => window.chatPerf.scrollToTop());
  await expect(page.locator('[data-turn-id="turn-1"]')).toBeVisible();
});

for (const mode of ["cap", "stress"] as const) {
  test(`${mode} chat performance envelope`, async ({ page }) => {
    test.setTimeout(180_000);
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/tests/ui/fixtures/chat-performance.html");
    await page.waitForFunction(() => Boolean(window.chatPerf));
    const mount = await page.evaluate(
      (kind) => window.chatPerf.mount(kind),
      mode,
    );
    if (mode === "cap")
      await expect(
        page.getByText("Worker 100-10 started working"),
      ).toBeAttached();
    await page.waitForTimeout(200);
    const stream = await page.evaluate(() => window.chatPerf.stream(30));
    const ordered = [...stream.durations].sort((left, right) => left - right);
    const p95 = ordered[Math.ceil(ordered.length * 0.95) - 1];
    console.log(JSON.stringify({ mode, mount, streamP95Ms: p95, stream }));
    expect(mount.commitCount).toBeLessThanOrEqual(6);
    expect(stream.commitCount).toBeLessThanOrEqual(32);
    if (mode === "cap") {
      expect(mount.renderedTurns).toBeLessThanOrEqual(40);
      expect(mount.domNodes).toBeLessThanOrEqual(3000);
      expect(mount.mountMs).toBeLessThanOrEqual(150);
      expect(p95).toBeLessThanOrEqual(40);
      expect(stream.heapBytes).toBeLessThanOrEqual(40_000_000);
    } else {
      expect(mount.renderedTurns).toBeLessThanOrEqual(40);
      expect(mount.domNodes).toBeLessThanOrEqual(4000);
      expect(mount.mountMs).toBeLessThanOrEqual(250);
      expect(p95).toBeLessThanOrEqual(50);
      expect(stream.heapBytes).toBeLessThanOrEqual(60_000_000);
    }
    await page.evaluate(() => window.chatPerf.scrollToTop());
    await expect(page.locator('[data-turn-id="turn-1"]')).toBeVisible();
    if (mode === "cap")
      await expect(page.getByText("Worker 1-8 started working")).toBeAttached();
    expect(await page.locator(".acp-turn").count()).toBeLessThanOrEqual(40);
  });
}
