import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();
const readArea = readFileSync(
  "src/features/browser/readAnnotateArea.js",
  "utf8",
).replace("export default ", "");

test("capture holds timer-driven and compositor-driven control movement", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/browser-annotate-drag.html");
  await page.evaluate(() => {
    document.body.innerHTML = `<style>
      @keyframes move { to { transform: translateX(200px); } }
      input { position:absolute; animation:move .08s infinite alternate linear; }
      </style><input value="FORM_VALUE_MUST_BE_MASKED">`;
    let top = false;
    setInterval(() => {
      document.querySelector("input")!.style.top = (top ? 20 : 200) + "px";
      top = !top;
    }, 1);
  });
  const cdp = await page.context().newCDPSession(page);
  const geometry = async () =>
    (
      await cdp.send("Runtime.evaluate", {
        expression:
          "JSON.stringify(document.querySelector('input').getBoundingClientRect().toJSON())",
        returnByValue: true,
      })
    ).result.value;
  const { playbackRate } = await cdp.send("Animation.getPlaybackRate");
  const first = await geometry();
  await expect.poll(geometry).not.toBe(first);
  try {
    await cdp.send("Input.setIgnoreInputEvents", { ignore: true });
    await cdp.send("Emulation.setScriptExecutionDisabled", { value: true });
    await cdp.send("Animation.setPlaybackRate", { playbackRate: 0 });
    const mask = await geometry();
    await cdp.send("Page.captureScreenshot", {
      format: "png",
      fromSurface: true,
      captureBeyondViewport: false,
    });
    // Wait beyond the complete animation period: neither movement source
    // may escape and return between the geometry reads around the screenshot.
    await new Promise((resolve) => setTimeout(resolve, 180));
    expect(await geometry()).toBe(mask);
  } finally {
    await cdp.send("Animation.setPlaybackRate", { playbackRate });
    await cdp.send("Emulation.setScriptExecutionDisabled", { value: false });
    await cdp.send("Input.setIgnoreInputEvents", { ignore: false });
    await cdp.detach();
  }
  const resumed = await page.locator("input").boundingBox();
  await expect
    .poll(() => page.locator("input").boundingBox())
    .not.toEqual(resumed);
});

test("scroll-driven control motion fails picture coverage under the capture hold", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/browser-annotate-drag.html");
  await page.evaluate(() => {
    document.body.innerHTML = `<style>
      body { height: 2400px; margin: 0; }
      input { position:fixed; top:30px; animation:move 1s linear both; animation-timeline:scroll(root); }
      @keyframes move { to { transform:translateX(200px); } }
      </style><input value="FORM_VALUE_MUST_BE_MASKED">`;
    window.scrollTo({ top: 1600, behavior: "smooth" });
  });
  const cdp = await page.context().newCDPSession(page);
  const { playbackRate } = await cdp.send("Animation.getPlaybackRate");
  try {
    await cdp.send("Input.setIgnoreInputEvents", { ignore: true });
    await cdp.send("Emulation.setScriptExecutionDisabled", { value: true });
    await cdp.send("Animation.setPlaybackRate", { playbackRate: 0 });
    const { executionContextId } = await cdp.send("Page.createIsolatedWorld", {
      frameId: (await cdp.send("Page.getFrameTree")).frameTree.frame.id,
      worldName: "sage-capture-test",
    });
    const result = await cdp.send("Runtime.evaluate", {
      expression: `(${readArea})({x:0,y:0,width:400,height:300})`,
      contextId: executionContextId,
      returnByValue: true,
    });
    expect(result.result.value.motionSafe).toBe(false);
    expect(result.result.value.nodes).not.toContainEqual(
      expect.objectContaining({ text: "FORM_VALUE_MUST_BE_MASKED" }),
    );
  } finally {
    await cdp.send("Animation.setPlaybackRate", { playbackRate });
    await cdp.send("Emulation.setScriptExecutionDisabled", { value: false });
    await cdp.send("Input.setIgnoreInputEvents", { ignore: false });
    await cdp.detach();
  }
});
