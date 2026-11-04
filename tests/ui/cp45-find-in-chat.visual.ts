import { expect, test } from "@playwright/test";
import {
  disposeJourney,
  focusPane,
  pane,
} from "./fixtures/agentTurnJourneyBrowser";
import { openNavigationChat } from "./fixtures/chatNavigationFixture";

test.afterEach(async ({ page }) => disposeJourney(page));

test("CP-45 says a long message is searched only as far as it is shown", async ({
  page,
}) => {
  const alpha = await openNavigationChat(page, 6, false, true);
  await alpha.locator(".composer textarea").focus();
  await page.keyboard.press("Control+f");
  const input = alpha.getByRole("searchbox", { name: "Search chat" });
  await input.fill("Hidden tail needle");
  await page.clock.runFor(250);
  await expect(alpha.locator(".chat-find [role=status]")).toHaveText(
    "0 results",
  );
  await expect(
    alpha.getByText("Long messages are searched as far as they are shown."),
  ).toBeVisible();
  await expect(
    alpha.getByRole("button", { name: "Next match" }),
  ).toBeDisabled();
  await input.press("Escape");
  await expect(input).toHaveCount(0);
});

test("CP-45 Ctrl+F after a click on transcript text opens in that pane only", async ({
  page,
}) => {
  const alpha = await openNavigationChat(page);
  await alpha.locator(".acp-message--user").first().click();
  await page.evaluate(() => (document.activeElement as HTMLElement)?.blur());
  await page.keyboard.press("Control+f");
  await expect(
    alpha.getByRole("searchbox", { name: "Search chat" }),
  ).toBeFocused();
  await expect(pane(page, "Beta").getByRole("searchbox")).toHaveCount(0);
});

test("CP-45 find stays labelled and readable in forced colours and reduced motion", async ({
  page,
}) => {
  const alpha = await openNavigationChat(page);
  await page.emulateMedia({ forcedColors: "active", reducedMotion: "reduce" });
  await alpha.locator(".composer textarea").focus();
  await page.keyboard.press("Control+f");
  const input = alpha.getByRole("searchbox", { name: "Search chat" });
  await input.fill("needle");
  await page.clock.runFor(250);
  await expect(alpha.locator(".chat-find [role=status]")).toHaveText(
    "1 / 6 results",
  );
  // The box overlays the pane header (s6/f0676), so the shot includes it.
  await expect(page.locator(".agent-chat-center")).toHaveScreenshot(
    "find-in-chat-forced-colors.png",
  );
  await input.press("Escape");
  expect(await page.evaluate(() => CSS.highlights.size)).toBe(0);
});

test("CP-45 scoped chat find counts, highlights, wraps and closes", async ({
  page,
}) => {
  const alpha = await openNavigationChat(page);
  await alpha.locator(".composer textarea").focus();
  await page.keyboard.press("Control+f");
  const input = alpha.getByRole("searchbox", { name: "Search chat" });
  await expect(input).toHaveAttribute("placeholder", "Search chat…");
  await expect(input).toBeFocused();
  await expect(pane(page, "Beta").getByRole("searchbox")).toHaveCount(0);
  await expect(page.locator(".agent-chat-center")).toHaveScreenshot(
    "find-in-chat-empty.png",
  );
  // The journey clock keeps running in real time; pause it so the 200 ms
  // debounce boundary is exact rather than racing the test runner.
  await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 1_000);
  await input.fill("needle");
  await page.clock.runFor(199);
  await expect(
    alpha.getByRole("status").filter({ hasText: "0 results" }),
  ).toBeVisible();
  await page.clock.runFor(1);
  const status = alpha.locator(".chat-find [role=status]");
  await expect(status).toHaveText("1 / 6 results");
  await page.clock.resume();
  await expect
    .poll(() =>
      page.evaluate(() => {
        const highlight = CSS.highlights.get("sage-find-current");
        const range = highlight && [...highlight][0];
        if (!range) return false;
        const bounds = range.getBoundingClientRect();
        const port = range.startContainer
          .parentElement!.closest(".message-list")!
          .getBoundingClientRect();
        return bounds.top >= port.top && bounds.bottom <= port.bottom;
      }),
    )
    .toBe(true);
  const colors = await page.evaluate(() => ({
    current: getComputedStyle(document.body, "::highlight(sage-find-current)")
      .backgroundColor,
    other: getComputedStyle(document.body, "::highlight(sage-find)")
      .backgroundColor,
    currentCount: CSS.highlights.get("sage-find-current")?.size,
    otherCount: CSS.highlights.get("sage-find")?.size,
  }));
  expect(colors).toEqual({
    current: "rgb(251, 106, 34)",
    other: "rgb(255, 210, 64)",
    currentCount: 1,
    otherCount: 5,
  });
  await expect(page.locator(".agent-chat-center")).toHaveScreenshot(
    "find-in-chat.png",
  );
  await input.press("Enter");
  await expect(status).toHaveText("2 / 6 results");
  await input.press("Shift+Enter");
  await input.press("Shift+F3");
  await expect(status).toHaveText("6 / 6 results");
  await input.press("Escape");
  await expect(input).toHaveCount(0);
  await expect(alpha.locator(".composer textarea")).toBeFocused();
  expect(await page.evaluate(() => CSS.highlights.size)).toBe(0);
});

test("CP-45 finds unmounted retained turns and truthfully reports partial history", async ({
  page,
}) => {
  const alpha = await openNavigationChat(page, 100, true);
  // Find searches a transcript; give Beta one Turn so its pane has one
  // (an empty chat shows the new-chat home and no transcript to search).
  await page.evaluate(async () => {
    const [
      { acpProjectionStoreFor },
      { appendTurn, updateLatestTurn, JOURNEY_IDS },
      { publishJourneyPage },
    ] = await Promise.all([
      import("/src/app/acpProjectionRegistry.ts"),
      import("/tests/ui/fixtures/agentTurnJourneyModel.ts"),
      import("/tests/ui/fixtures/agentTurnJourneyFixture.ts"),
    ]);
    const initial = acpProjectionStoreFor(JOURNEY_IDS.beta).getState().page;
    if (!initial) throw new Error("Beta projection not ready");
    publishJourneyPage(
      updateLatestTurn(
        appendTurn(initial, "Beta prompt", "completed"),
        "completed",
        "Beta answer.",
      ),
    );
  });
  // Focus panes the way a reader does: activating a chat is a native
  // transition, so wait for it before typing.
  const beta = pane(page, "Beta");
  await focusPane(page, "Beta");
  await expect(beta.locator(".acp-turn")).toHaveCount(1);
  await focusPane(page, "Alpha");
  await expect(alpha.locator(".acp-turn").last()).toBeVisible();
  await expect(alpha.locator('[data-turn-ordinal="12"]')).toHaveCount(0);
  await page.keyboard.press("Meta+f");
  const input = alpha.getByRole("searchbox", { name: "Search chat" });
  await input.fill("Needle turn 12.");
  await page.clock.runFor(250);
  await expect(alpha.locator(".chat-find [role=status]")).toHaveText(
    "1 result",
  );
  await expect(alpha.getByText(/Only loaded turns are searched/)).toBeVisible();
  await expect(alpha.locator('[data-turn-ordinal="12"]')).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(() =>
        [...(CSS.highlights.get("sage-find-current") ?? [])][0]?.toString(),
      ),
    )
    .toBe("Needle turn 12.");
  // AP-A1: selecting Beta replaces Alpha in the one centre pane, so Beta's
  // find starts empty and Alpha's highlights do not leak into it.
  await focusPane(page, "Beta");
  await expect(alpha).toHaveCount(0);
  await page.keyboard.press("Control+f");
  const betaInput = beta.getByRole("searchbox", { name: "Search chat" });
  await expect(betaInput).toBeVisible();
  await expect(betaInput).toHaveValue("");
  expect(
    await page.evaluate(
      () => CSS.highlights.get("sage-find-current")?.size ?? 0,
    ),
  ).toBe(0);
  await beta.getByRole("button", { name: "Close search" }).click();
  await expect(betaInput).toHaveCount(0);
  expect(await page.evaluate(() => CSS.highlights.size)).toBe(0);
});
