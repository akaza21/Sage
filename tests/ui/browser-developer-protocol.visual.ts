import { expect, test } from "@playwright/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

test("detaching privileged async work preserves the ordinary page session", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/browser-developer.html");
  const ordinary = await page.context().newCDPSession(page);
  const privileged = await page.context().newCDPSession(page);
  try {
    // Pin an object through the protocol, independently of the pending promise.
    // An otherwise unreferenced promise and its reactions can be collected.
    const pinned = await privileged.send("Runtime.evaluate", {
      expression:
        "(() => { const held = { bytes: new ArrayBuffer(1 << 20) }; globalThis.__pendingDeveloper = new WeakRef(held); return held; })()",
      objectGroup: "developer-lifetime-regression",
      returnByValue: false,
    });
    expect(pinned.result.objectId).toBeTruthy();
    // V8's timeout covers synchronous evaluation, not an unresolved promise.
    const pending = privileged
      .send("Runtime.evaluate", {
        expression: "new Promise(() => {})",
        awaitPromise: true,
        returnByValue: true,
        timeout: 100,
      })
      .then(
        () => "completed",
        () => "detached",
      );
    await expect
      .poll(async () => {
        const result = await ordinary.send("Runtime.evaluate", {
          expression: "Boolean(globalThis.__pendingDeveloper)",
          returnByValue: true,
        });
        return result.result.value;
      })
      .toBe(true);
    await ordinary.send("HeapProfiler.collectGarbage");
    const retained = await ordinary.send("Runtime.evaluate", {
      expression: "globalThis.__pendingDeveloper.deref() !== undefined",
      returnByValue: true,
    });
    expect(retained.result.value).toBe(true);
    await privileged.detach();
    await expect(pending).resolves.toBe("detached");
    await ordinary.send("HeapProfiler.collectGarbage");
    const released = await ordinary.send("Runtime.evaluate", {
      expression: "globalThis.__pendingDeveloper.deref() === undefined",
      returnByValue: true,
    });
    expect(released.result.value).toBe(true);
    const value = await ordinary.send("Runtime.evaluate", {
      expression: "6 * 7",
      returnByValue: true,
    });
    expect(value.result.value).toBe(42);
  } finally {
    await privileged.detach().catch(() => {});
    await ordinary.detach();
  }
});
