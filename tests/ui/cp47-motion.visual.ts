import { expect, test, type Locator, type Page } from "@playwright/test";
import os from "node:os";
import path from "node:path";
import { build } from "vite";

import {
  disposeJourney,
  openJourney,
  pane,
  runEvent,
} from "./fixtures/agentTurnJourneyBrowser";
import { settleTranscriptAtEnd } from "./shared/visualBrowserPrimitives";

/*
 * CP-47: Codex live motion parity (parity spec section 10). CSS animations do
 * not follow page.clock, so timing checks pause each Web Animation and seek
 * it with currentTime instead of waiting on wall time.
 */

const output = path.join(
  os.tmpdir(),
  `sage-cp47-motion-${process.env.SAGE_UI_PORT ?? "default"}-${process.pid}`,
);

test.beforeAll(async () => {
  await build({
    configFile: false,
    publicDir: false,
    root: path.resolve("tests/ui/fixtures"),
    build: {
      outDir: output,
      emptyOutDir: true,
      rolldownOptions: {
        input: path.resolve("tests/ui/fixtures/activity-parity.html"),
      },
    },
  });
});

async function openActivity(page: Page, scenario: string) {
  await page.route("https://activity.test/**", async (route) => {
    const pathname = new URL(route.request().url()).pathname;
    const file = path.resolve(
      output,
      `.${pathname === "/" ? "/activity-parity.html" : pathname}`,
    );
    if (!file.startsWith(output + path.sep))
      throw new Error("Escaping fixture request");
    await route.fulfill({ path: file });
  });
  await page.goto(`https://activity.test/?scenario=${scenario}`);
  await expect(page.locator(".acp-turn")).toBeVisible();
}

/** Seeks the element's named CSS animation and reads one computed property. */
async function sampleAt(
  target: Locator,
  animationName: string,
  times: number[],
  property: string,
) {
  return target.evaluate(
    (node, { animationName, times, property }) => {
      const animation = node
        .getAnimations()
        .find(
          (candidate) =>
            (candidate as CSSAnimation).animationName === animationName,
        );
      if (!animation) throw new Error(`No ${animationName} animation`);
      animation.pause();
      return times.map((time) => {
        animation.currentTime = time;
        return getComputedStyle(node).getPropertyValue(property);
      });
    },
    { animationName, times, property },
  );
}

test.describe("CP-47 motion", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test("shimmer sweeps left to right in 600 ms, then idles, on a 4 s period", async ({
    page,
  }) => {
    await openActivity(page, "running");
    const label = page.locator(".acp-tool-rollup .acp-live-shimmer");
    await expect(label).toHaveCSS("animation-duration", "4s");
    const [start, mid, end, idle] = await sampleAt(
      label,
      "acp-live-shimmer",
      [0, 300, 600, 1000],
      "background-position-x",
    );
    const percent = (value: string) => Number.parseFloat(value);
    expect(percent(start)).toBeCloseTo(100, 0);
    expect(percent(mid)).toBeCloseTo(50, 0);
    // A falling position on a 300% image moves the band rightwards.
    expect(percent(mid)).toBeLessThan(percent(start));
    expect(percent(end)).toBeCloseTo(0, 0);
    expect(idle).toBe(end);
    // The live header never shimmers (Codex s8 header stays #8b8b8b).
    await expect(page.locator(".chat-transcript-work__heading")).toHaveCSS(
      "animation-name",
      "none",
    );
  });

  test("streamed blocks fade in once on opacity, and settled answers do not", async ({
    page,
  }) => {
    await openActivity(page, "thinking");
    const block = page
      .locator(".acp-message--assistant .acp-markdown > *")
      .first();
    await expect(block).toHaveCSS("animation-name", "acp-stream-in");
    await expect(block).toHaveCSS("animation-duration", "0.3s");
    const [first, half, done] = await sampleAt(
      block,
      "acp-stream-in",
      [0, 150, 300],
      "opacity",
    );
    expect(Number(first)).toBeCloseTo(0.15, 2);
    expect(Number(half)).toBeGreaterThan(0.15);
    expect(Number(done)).toBe(1);
    const transform = await block.evaluate(
      (node) => getComputedStyle(node).transform,
    );
    expect(transform).toBe("none");

    await openActivity(page, "simple");
    await expect(
      page.locator(".acp-message--assistant .acp-markdown > *").first(),
    ).toHaveCSS("animation-name", "none");
  });

  test("Worked rows stagger in top to bottom and collapse instantly", async ({
    page,
  }) => {
    // Subagent rows stay unmounted until Worked opens, then fade in.
    await openActivity(page, "subagents");
    const worked = page.locator("details.chat-transcript-work");
    await worked.locator("summary").click();
    const rows = page.locator(
      ".chat-transcript-work[open] + .chat-transcript-work__groups > :not(.acp-tool-rollup)",
    );
    await expect(rows.first()).toHaveCSS("animation-name", "sage-row-in");
    const delays = await rows.evaluateAll((nodes) =>
      nodes.map((node) => getComputedStyle(node).animationDelay),
    );
    expect(delays.length).toBeGreaterThan(0);
    for (let index = 1; index < delays.length; index++)
      expect(Number.parseFloat(delays[index])).toBeGreaterThanOrEqual(
        Number.parseFloat(delays[index - 1]),
      );
    await expect(worked.locator(".chat-transcript-work__chevron")).toHaveCSS(
      "transition-duration",
      "0.12s",
    );
    await worked.locator("summary").click();
    await expect(rows).toHaveCount(0);
  });

  test("reduced motion leaves no running animation after settle", async ({
    page,
  }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openActivity(page, "thinking");
    await page.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    );
    expect(
      await page.evaluate(
        () =>
          document
            .getAnimations()
            .filter((animation) => animation.playState === "running").length,
      ),
    ).toBe(0);
  });
});

test.describe("CP-47 chat motion", () => {
  test.afterEach(async ({ page }) => disposeJourney(page));

  test("jump control is a 28 px circle centred on the column, and fades out", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1936, height: 1048 });
    await openJourney(page);
    await runEvent(page, { type: "seedHistory", chat: "alpha", turns: 12 });
    await settleTranscriptAtEnd(page);
    const alpha = pane(page, "Alpha");
    const transcript = alpha.locator(".message-list");
    await transcript.evaluate((node) => {
      node.scrollTop = 0;
      node.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    const button = alpha.getByRole("button", { name: "Jump to latest" });
    await expect(button).toBeVisible();
    const [buttonBox, columnBox, composerBox] = await Promise.all([
      button.boundingBox(),
      alpha.locator(".message-list__content").boundingBox(),
      alpha.locator("form.composer").boundingBox(),
    ]);
    expect(buttonBox!.width).toBe(28);
    expect(buttonBox!.height).toBe(28);
    const centre = (box: { x: number; width: number }) => box.x + box.width / 2;
    expect(
      Math.abs(centre(buttonBox!) - centre(columnBox!)),
    ).toBeLessThanOrEqual(2);
    expect(buttonBox!.y + buttonBox!.height).toBeLessThan(composerBox!.y);
    await expect(button).toHaveCSS("transition-duration", /0\.15s/);

    await button.click();
    // The control stays mounted, hides, and keeps the transcript height.
    await expect(button).toBeHidden();
    await expect(alpha.locator(".chat-follow-end")).toHaveCount(1);
    await expect
      .poll(() =>
        transcript.evaluate(
          (node) => node.scrollHeight - node.scrollTop - node.clientHeight,
        ),
      )
      .toBeLessThanOrEqual(1);
  });

  test("jump to latest travels to the end over about 320 ms, easing out (s2/f0046-f0048)", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await openJourney(page);
    await runEvent(page, { type: "seedHistory", chat: "alpha", turns: 12 });
    await settleTranscriptAtEnd(page);
    const alpha = pane(page, "Alpha");
    const transcript = alpha.locator(".message-list");
    await transcript.evaluate((node) => {
      node.scrollTop = 0;
      node.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    await expect(
      alpha.getByRole("button", { name: "Jump to latest" }),
    ).toBeVisible();
    // Click and sample scrollTop every frame for 600 ms in the page itself.
    const samples = await transcript.evaluate(
      (node) =>
        new Promise<{ at: number; top: number; end: number }[]>((resolve) => {
          const button = node
            .closest(".agent-chat-pane, .chat-surface, body")!
            .querySelector<HTMLButtonElement>(
              'button[aria-label="Jump to latest"]',
            )!;
          const start = performance.now();
          const out: { at: number; top: number; end: number }[] = [];
          button.click();
          const tick = (now: number) => {
            out.push({
              at: now - start,
              top: node.scrollTop,
              end: node.scrollHeight - node.clientHeight,
            });
            if (now - start < 600) requestAnimationFrame(tick);
            else resolve(out);
          };
          requestAnimationFrame(tick);
        }),
    );
    const end = samples.at(-1)!.end;
    const progress = (at: number) =>
      samples.filter((sample) => sample.at <= at).at(-1)!.top / end;
    // Visibly travelling, not an instant jump; ease-out runs ahead of linear.
    expect(progress(60)).toBeLessThan(0.9);
    expect(progress(100)).toBeGreaterThan(0.1);
    expect(progress(200)).toBeGreaterThan(progress(100));
    expect(progress(200)).toBeLessThan(1);
    expect(end - samples.at(-1)!.top).toBeLessThanOrEqual(1);
    const arrived = samples.find((sample) => sample.end - sample.top <= 1)!;
    expect(arrived.at).toBeGreaterThanOrEqual(250);
    expect(arrived.at).toBeLessThanOrEqual(450);
  });

  test("a multi-chunk replay fades only the newest appended run (s5/f0309)", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await openJourney(page);
    const alpha = pane(page, "Alpha");
    await alpha
      .getByRole("textbox", { name: "Message agent" })
      .fill("Stream a short answer");
    await alpha.getByRole("button", { name: "Send message" }).click();
    await runEvent(page, {
      type: "resolveSend",
      chat: "alpha",
      state: "sending",
    });
    const chunks = [
      "Top hit:",
      " Streaming API",
      " responses explain",
      " typed events.",
    ];
    const answer = alpha.locator(".acp-message--assistant .acp-markdown p");
    const tail = alpha.locator(".acp-message--assistant .acp-stream-new");
    for (const [index, streamDelta] of chunks.entries()) {
      await runEvent(page, {
        type: "publish",
        chat: "alpha",
        state: "running",
        streamDelta,
      });
      await expect(answer).toHaveText(chunks.slice(0, index + 1).join(""));
      if (index === 0) {
        await expect(tail).toHaveCount(0);
        continue;
      }
      // Only the newest run is marked, and it fades on the stream curve.
      await expect(tail).toHaveCount(1);
      await expect(tail).toHaveText(streamDelta);
      await expect(tail).toHaveCSS("animation-name", "acp-stream-in");
      const [first, done] = await sampleAt(
        tail,
        "acp-stream-in",
        [0, 300],
        "opacity",
      );
      expect(Number(first)).toBeCloseTo(0.15, 2);
      expect(Number(done)).toBe(1);
    }
    // Earlier text sits outside the faded run, fully opaque.
    expect(
      await answer.evaluate((node) => getComputedStyle(node).opacity),
    ).toBe("1");
    await runEvent(page, {
      type: "publish",
      chat: "alpha",
      state: "completed",
    });
    await expect(tail).toHaveCount(0);
    await expect(answer).toHaveText(chunks.join(""));
  });

  test("new activity below a paused reader shows the dots pill, which jumps to latest", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await openJourney(page);
    await runEvent(page, { type: "seedHistory", chat: "alpha", turns: 8 });
    await settleTranscriptAtEnd(page);
    const alpha = pane(page, "Alpha");
    const composer = alpha.getByRole("textbox", { name: "Message agent" });
    await composer.fill("Stream while I read");
    await alpha.getByRole("button", { name: "Send message" }).click();
    await runEvent(page, {
      type: "resolveSend",
      chat: "alpha",
      state: "sending",
    });
    const transcript = alpha.locator(".message-list");
    await transcript.hover();
    await page.mouse.wheel(0, -240);
    await runEvent(page, {
      type: "publish",
      chat: "alpha",
      state: "running",
      assistantText: "New rows land below the fold.\n\nAnd another block.",
    });
    const pill = alpha.getByRole("button", {
      name: "New activity below. Jump to latest",
    });
    await expect(pill).toBeVisible();
    // The three dots are the button's ::before, its arrow icon restyled as the
    // middle dot, and its ::after (chat-follow-end.css); each bounces on its
    // own delay, left to right.
    await expect(pill.locator("svg")).toHaveCount(1);
    const dots = await pill.evaluate((node) => {
      const icon = node.querySelector("svg")!;
      return [
        getComputedStyle(node, "::before"),
        getComputedStyle(icon),
        getComputedStyle(node, "::after"),
      ].map((style) => ({
        name: style.animationName,
        delay: style.animationDelay,
        width: style.width,
        height: style.height,
      }));
    });
    expect(dots.map((dot) => dot.name)).toEqual(
      Array(3).fill("sage-dot-bounce"),
    );
    expect(dots.map((dot) => dot.delay)).toEqual(["0s", "0.125s", "0.25s"]);
    for (const dot of dots) {
      expect(dot.width).toBe("4px");
      expect(dot.height).toBe("4px");
    }
    // The paused reader stays where it scrolled once the wheel settles.
    await page.waitForTimeout(400);
    await expect(pill).toBeVisible();
    expect(
      await transcript.evaluate(
        (node) => node.scrollHeight - node.scrollTop - node.clientHeight,
      ),
    ).toBeGreaterThan(64);
    await pill.click();
    await expect(pill).toBeHidden();
    await expect
      .poll(() =>
        transcript.evaluate(
          (node) => node.scrollHeight - node.scrollTop - node.clientHeight,
        ),
      )
      .toBeLessThanOrEqual(1);
  });

  test("send fill cross-fades on opacity in 150 ms and popovers open without a translate", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await openJourney(page);
    const alpha = pane(page, "Alpha");
    const send = alpha.getByRole("button", { name: "Send message" });
    const layer = (property: string) =>
      send.evaluate(
        (node, name) =>
          getComputedStyle(node, "::before").getPropertyValue(name),
        property,
      );
    await alpha.getByRole("textbox", { name: "Message agent" }).fill("");
    await expect(send).toBeDisabled();
    await expect.poll(() => layer("opacity")).toBe("1");
    expect(await layer("transition-duration")).toBe("0.15s");
    expect(await layer("transition-property")).toBe("opacity");
    await alpha.getByRole("textbox", { name: "Message agent" }).fill("Hi");
    await expect(send).toBeEnabled();
    await expect.poll(() => layer("opacity")).toBe("0");

    // The journey has no composer options, so open the harness picker, which
    // shares the menu popover surface.
    await alpha.getByRole("button", { name: "Harness", exact: true }).click();
    const menu = page.locator(".menu-popover:not([hidden])").first();
    await expect(menu).toBeVisible();
    await expect(menu).toHaveCSS("animation-name", "menu-popover-enter");
    await expect(menu).toHaveCSS("animation-duration", "0.1s");
    const keyframes = await menu.evaluate((node) =>
      node
        .getAnimations()
        .flatMap((animation) =>
          (animation.effect as KeyframeEffect).getKeyframes(),
        ),
    );
    expect(keyframes.some((frame) => "transform" in frame)).toBe(false);
  });
});
