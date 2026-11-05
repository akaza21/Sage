import { expect, type Page, type TestInfo } from "@playwright/test";

import type { JourneyEvent } from "./agentTurnJourneyFixture";
import type { TurnState } from "../../../src/domain/acp";
import { turnStateLabels } from "../../../src/features/chat/acp/statusSemantics";

const FIXED_TIME = "2026-09-04T12:00:00.000Z";
const MAX_SCREENSHOT_ATTACHMENTS = 12;
// The header shares Chat status with the rail; individual Turns retain their
// protocol state labels below. These are the chat-surface display contracts.
const chatStatusLabels: Record<TurnState, string> = {
  queued: "Working…",
  sending: "Working…",
  running: "Working…",
  waitingModel: "Working…",
  waitingTool: "Working…",
  waitingApproval: "Awaiting approval",
  waitingUser: "Waiting",
  cancelling: "Working…",
  completed: "Done",
  interrupted: "Done",
  cancelled: "Done",
  failed: "Failed",
  deliveryUnknown: "Failed",
};
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
  // AP-A1: the centre shows one Chat; another Chat is reached by selecting
  // it, never by an automatic second pane.
  await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
  await expect(
    page.getByRole("region", { name: "Chat pane Beta" }),
  ).toHaveCount(0);
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
    const result = await fixture.runJourneyEvent(nextEvent);
    return result;
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

export async function expectTurnState(
  page: Page,
  owner: ReturnType<typeof pane>,
  ordinal: number,
  state: TurnState,
) {
  const turn = owner.getByRole("region", {
    name: "Turn " + ordinal,
    exact: true,
  });
  await expect(turn).toBeVisible();
  await expect(turn).toHaveAttribute("data-turn-state", state);
  await expect(turn.locator("[data-turn-status]")).toHaveText(
    turnStateLabels[state],
  );
  if (
    (await owner.getAttribute("class"))?.includes("agent-chat-pane--focused")
  ) {
    await expect(
      page.locator(".agent-task-header").getByRole("status"),
    ).toHaveText(chatStatusLabels[state]);
  }
}

export async function expectRunningComposerReadyToQueue(
  owner: ReturnType<typeof pane>,
) {
  const composer = owner.getByRole("textbox", { name: "Message agent" });
  await expect(composer).toBeEnabled();
  // Wave 5 (Codex): the placeholder stays "Do anything"; queueing is
  // announced through the accessible description below.
  await expect(composer).toHaveAttribute("placeholder", "Do anything");
  await expect(composer).toHaveAccessibleDescription(
    /Enter queues a message while this turn runs/,
  );
  await expect(
    owner.getByRole("button", { name: "Harness", exact: true }),
  ).toBeDisabled();
  await expect(
    owner.getByRole("button", { name: "Model", exact: true }),
  ).toBeDisabled();
  await expect(owner.locator(".composer__disabled-reason")).toHaveCount(0);
}

export async function focusPane(page: Page, name: "Alpha" | "Beta") {
  const target = pane(page, name);
  await page
    .getByRole("complementary", { name: "Projects and chats" })
    .getByRole("button", {
      name: new RegExp("^Open chat " + name + "(?:, pinned)?$"),
    })
    .click();
  await expect(target).toHaveClass(/agent-chat-pane--focused/);
  await expect(target.locator("form.composer")).toHaveAttribute(
    "aria-busy",
    "false",
  );
  await expect(
    target.getByRole("region", { name: "Agent chat " + name }),
  ).toBeVisible();
  const composer = target.getByRole("textbox", { name: "Message agent" });
  if (await composer.isEnabled()) {
    await composer.focus();
    await expect(composer).toBeFocused();
  } else {
    const cancel = target.getByRole("button", { name: "Cancel turn" });
    await expect(cancel).toBeEnabled();
    await cancel.focus();
    await expect(cancel).toBeFocused();
  }
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
