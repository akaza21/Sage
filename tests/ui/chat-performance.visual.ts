import { expect, test, type Page } from "@playwright/test";

import type { SubagentPaneTarget } from "../../src/app/subagentPaneStore";
import { settleRendering } from "./shared/visualBrowserPrimitives";

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
      openedSubagent: () => SubagentPaneTarget | null;
    };
  }
}

function capSubagentGroup(page: Page, turn: number) {
  return page
    .locator(`[data-turn-id="turn-${turn}"]`)
    .locator(".acp-subagent-row")
    .filter({
      has: page.getByRole("status", {
        name: `Worker ${turn}-8, Worker ${turn}-9 and Worker ${turn}-10 started working`,
        exact: true,
      }),
    });
}

async function settleTurnContent(page: Page) {
  await expect(
    page.getByRole("status").filter({ hasText: /^Loading /u }),
  ).toHaveCount(0);
  await settleRendering(page);
}

async function expectCapWorkerReachable(
  page: Page,
  turn: number,
  ordinal: number,
) {
  const group = capSubagentGroup(page, turn);
  // Completed Turns start collapsed under "Worked for" and do not mount the
  // grouped Subagent row until that disclosure is opened.
  await expect(group).not.toBeAttached();
  const work = page
    .locator(`[data-turn-id="turn-${turn}"]`)
    .locator("details.chat-transcript-work");
  await expect(work).toHaveCount(1);
  await expect(work).not.toHaveAttribute("open", "");
  await work.locator(":scope > summary").click();
  await expect(work).toHaveAttribute("open", "");
  await expect(group).toBeVisible();
  await expect(group.getByRole("status")).toHaveText(
    `Worker ${turn}-8, Worker ${turn}-9 and Worker ${turn}-10 started working`,
  );
  // Each grouped name is its own target; there is no disclosure to open.
  const worker = group.getByRole("button", {
    name: `Open subagent Worker ${turn}-${ordinal}`,
    exact: true,
  });
  await expect(worker).toBeVisible();
  await expect(worker).toBeEnabled();
  await worker.click();
  await expect
    .poll(() => page.evaluate(() => window.chatPerf.openedSubagent()))
    .toEqual({
      owner: {
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
      },
      itemId: `item-${turn}-${ordinal}`,
      agentKey: `worker-${turn}-${ordinal}`,
    });
  await work.locator(":scope > summary").click();
  await expect(work).not.toHaveAttribute("open", "");
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
  await settleTurnContent(page);
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
    .toBeLessThanOrEqual(1);
});

test("expanding a measured Turn keeps neighboring history reachable", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/tests/ui/fixtures/chat-performance.html");
  await page.waitForFunction(() => Boolean(window.chatPerf));
  await page.evaluate(() => window.chatPerf.mount("cap"));
  // The text renderer loads on demand. Choose the expansion anchor after
  // its content replaces the loading placeholders, not during that reflow.
  await page.waitForFunction(() => {
    const messages = [...document.querySelectorAll(".acp-message")];
    return (
      messages.length > 0 &&
      messages.every((message) => message.querySelector(".acp-markdown"))
    );
  });
  await page.evaluate(() => window.chatPerf.scrollToMiddle());
  await page.waitForFunction(() => Boolean(window.chatPerf.visibleAnchor()));
  await settleTurnContent(page);
  const anchored = await page.evaluate(() => window.chatPerf.visibleAnchor());
  const disclosure = page
    .locator(`[data-turn-id="${anchored?.id}"] details`)
    .first();
  await expect(disclosure).toBeVisible();
  // Bring the summary fully into view first so the click's own actionability
  // scroll cannot masquerade as anchor drift caused by the expansion.
  // Align it to the top: collapsed Turns are short enough that centering the
  // summary would hand the anchor to an earlier Turn.
  const summary = disclosure.locator(":scope > summary");
  await summary.evaluate((element) =>
    element.scrollIntoView({ block: "start" }),
  );
  await page.evaluate(
    () =>
      new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      ),
  );
  const before = await page.evaluate(() => window.chatPerf.visibleAnchor());
  expect(before?.id).toBe(anchored?.id);
  await summary.click();
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
        page
          .locator('[data-turn-id="turn-100"]')
          .locator("details.chat-transcript-work"),
      ).toHaveCount(1);
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
    if (mode === "cap") await expectCapWorkerReachable(page, 100, 10);
    await page.evaluate(() => window.chatPerf.scrollToTop());
    await expect(page.locator('[data-turn-id="turn-1"]')).toBeVisible();
    if (mode === "cap") await expectCapWorkerReachable(page, 1, 8);
    expect(await page.locator(".acp-turn").count()).toBeLessThanOrEqual(40);
  });
}
