import { expect, type Page, type TestInfo } from "@playwright/test";

import type { JourneyEvent } from "./agentTurnJourneyFixture";

const FIXED_TIME = "2026-09-04T12:00:00.000Z";
const MAX_SCREENSHOT_ATTACHMENTS = 12;
export const JOURNEY_IDS = {
  projectA: "journey-project-a",
  projectB: "journey-project-b",
  alpha: "journey-chat-alpha",
  beta: "journey-chat-beta",
  gamma: "journey-chat-gamma",
} as const;

export async function openJourney(page: Page) {
  await page.clock.install({ time: new Date(FIXED_TIME) });
  await page.addInitScript(() => {
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 264,
        agentChatSplitPercent: 50,
        windowMode: "agent",
      }),
    );
  });
  await page.goto("/");
  await expect(page.locator("main.workbench")).toBeVisible();
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/agentTurnJourneyFixture.ts");
    await fixture.installAgentTurnJourneyFixture();
  });
  await expect(page.locator("html")).toHaveAttribute(
    "data-turn-journey-fixture",
    "ready",
  );
  await expect(
    page.getByRole("region", { name: "Chat pane Alpha" }),
  ).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Chat pane Beta" }),
  ).toBeVisible();
}

export async function settleRender(page: Page, projectionEvent = false) {
  if (projectionEvent) await page.clock.runFor(60);
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
}

export async function runEvent(page: Page, event: JourneyEvent) {
  const result = await page.evaluate(async (nextEvent) => {
    const fixture =
      await import("/tests/ui/fixtures/agentTurnJourneyFixture.ts");
    return fixture.runJourneyEvent(nextEvent);
  }, event);
  await settleRender(page, true);
  return result;
}

export async function audit(page: Page) {
  return page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/agentTurnJourneyFixture.ts");
    return fixture.journeyAudit();
  });
}

export async function expectRequestCount(
  page: Page,
  kind: "sendRequests" | "cancelRequests",
  count: number,
) {
  await expect.poll(async () => (await audit(page))[kind].length).toBe(count);
}

export function screenshotEvidence(testInfo: TestInfo, page: Page) {
  let count = 0;
  return async (label: string) => {
    if (count >= MAX_SCREENSHOT_ATTACHMENTS) {
      throw new Error(
        `Screenshot attachment limit ${MAX_SCREENSHOT_ATTACHMENTS} exceeded`,
      );
    }
    count += 1;
    await testInfo.attach(`${String(count).padStart(2, "0")}-${label}`, {
      body: await page.locator("main.workbench").screenshot({
        animations: "disabled",
        caret: "hide",
      }),
      contentType: "image/png",
    });
  };
}

export function pane(page: Page, name: "Alpha" | "Beta" | "Gamma") {
  return page.getByRole("region", { name: `Chat pane ${name}` });
}

export function turnStatus(
  owner: ReturnType<typeof pane>,
  ordinal: number,
  name: string,
) {
  return owner
    .getByRole("region", { name: `Turn ${ordinal}` })
    .getByRole("status", { name });
}

export async function focusPane(page: Page, name: "Alpha" | "Beta") {
  const target = pane(page, name);
  const focusButton = target.getByRole("button", {
    name: new RegExp(`${name}$`),
  });
  await focusButton.focus();
  await expect(focusButton).toHaveAttribute("aria-pressed", "true");
  await expect(target.locator("form.composer")).toHaveAttribute(
    "aria-busy",
    "false",
  );
  await expect(
    target.getByRole("region", { name: `Agent chat ${name}` }),
  ).toBeVisible();
  await settleRender(page);
  return target;
}

export async function disposeJourney(page: Page) {
  await page
    .evaluate(async () => {
      const fixture =
        await import("/tests/ui/fixtures/agentTurnJourneyFixture.ts");
      await fixture.disposeAgentTurnJourneyFixture();
    })
    .catch(() => undefined);
}
