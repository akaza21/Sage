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
    };
  }
}

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
    const stream = await page.evaluate(() => window.chatPerf.stream(30));
    const ordered = [...stream.durations].sort((left, right) => left - right);
    const p95 = ordered[Math.ceil(ordered.length * 0.95) - 1];
    console.log(JSON.stringify({ mode, mount, streamP95Ms: p95, stream }));
    expect(mount.commitCount).toBe(1);
    expect(stream.commitCount).toBe(30);
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
  });
}
