import { expect, test } from "@playwright/test";

import {
  audit,
  expectRunningComposerReadyToQueue,
  focusPane,
  JOURNEY_IDS,
  openJourney,
  pane,
  runEvent,
  screenshotEvidence,
  expectTurnState,
} from "./agentTurnJourneyBrowser";
import {
  expectContinuityIdentityEvidence,
  expectStaleIdentityEvidence,
} from "./agentTurnJourneyEvidence";
import { MAX_SCRIPTED_EVENTS } from "./agentTurnJourneyLimits";

export function registerAgentTurnContinuityScenarios() {
  test("terminal follow-up, Jump to latest, and background unread stay truthful", async ({
    page,
  }, testInfo) => {
    await openJourney(page);
    const evidence = screenshotEvidence(testInfo, page);
    const alpha = pane(page, "Alpha");
    const composer = alpha.getByRole("textbox", { name: "Message agent" });

    await composer.fill("First terminal Turn");
    await alpha.getByRole("button", { name: "Send message" }).click();
    await runEvent(page, {
      type: "resolveSend",
      chat: "alpha",
      state: "sending",
    });
    await runEvent(page, {
      type: "publish",
      chat: "alpha",
      state: "completed",
      assistantText: "First Turn completed authoritatively.",
    });
    await expectTurnState(page, alpha, 1, "completed");
    await expect(composer).toBeEnabled();
    await evidence("first-turn-completed");

    await composer.fill("Explicit follow-up");
    await alpha.getByRole("button", { name: "Send message" }).click();
    await runEvent(page, {
      type: "resolveSend",
      chat: "alpha",
      state: "sending",
    });
    await expectTurnState(page, alpha, 2, "sending");
    await runEvent(page, {
      type: "publish",
      chat: "alpha",
      state: "completed",
      assistantText: "Follow-up completed.",
    });
    await evidence("follow-up-completed");

    await runEvent(page, { type: "seedHistory", chat: "alpha", turns: 8 });
    const transcript = alpha.locator(".message-list");
    await transcript.hover();
    await page.mouse.wheel(0, -5_000);
    await expect(
      alpha.getByRole("button", { name: "Jump to latest" }),
    ).toBeVisible();
    await evidence("follow-end-paused");

    await composer.fill("Latest while scrolled away");
    await alpha.getByRole("button", { name: "Send message" }).click();
    await runEvent(page, {
      type: "resolveSend",
      chat: "alpha",
      state: "sending",
    });
    await expect(
      alpha.getByRole("button", { name: "Jump to latest" }),
    ).toBeVisible();
    const latestMessage = alpha.getByText("Latest while scrolled away").last();
    await expect(latestMessage).not.toBeInViewport();
    await alpha.getByRole("button", { name: "Jump to latest" }).click();
    await expect(
      alpha.getByRole("button", { name: "Jump to latest" }),
    ).toHaveCount(0);
    await expect(latestMessage).toBeInViewport();
    await expect
      .poll(() =>
        transcript.evaluate(
          (node) => node.scrollHeight - node.scrollTop - node.clientHeight,
        ),
      )
      .toBeLessThanOrEqual(1);
    await evidence("jumped-to-latest");

    const beta = await focusPane(page, "Beta");
    const betaComposer = beta.getByRole("textbox", { name: "Message agent" });
    await betaComposer.fill("Background completion");
    await beta.getByRole("button", { name: "Send message" }).click();
    await runEvent(page, {
      type: "resolveSend",
      chat: "beta",
      state: "sending",
    });
    await runEvent(page, { type: "publish", chat: "beta", state: "running" });
    await focusPane(page, "Alpha");
    await runEvent(page, {
      type: "publish",
      chat: "beta",
      state: "completed",
      assistantText: "Background result is complete.",
    });
    expect((await audit(page)).workbench).toMatchObject({
      activeTabId: JOURNEY_IDS.alpha,
      pinnedAgentChatOwner: JOURNEY_IDS.alpha,
      betaSeenFingerprint: "Running:Background completion",
    });
    const betaRailRow = page.locator(".project-chat-row").filter({
      has: page.getByRole("button", {
        name: /^Open chat Beta(?:, pinned)?$/,
      }),
    });
    await expect(betaRailRow.getByLabel("Unread updates")).toBeVisible();
    await expect(
      beta.getByText("Background result is complete.", { exact: true }),
    ).toBeVisible();
    await evidence("background-completion-unread");
    await focusPane(page, "Beta");
    await expect(betaRailRow.getByLabel("Unread updates")).toHaveCount(0);

    const result = await audit(page);
    expect(result.eventCount).toBeLessThanOrEqual(MAX_SCRIPTED_EVENTS);
    expect(
      result.sendRequests.filter(
        (request) => request.chatId === JOURNEY_IDS.alpha,
      ),
    ).toHaveLength(3);
    expect(
      result.sendRequests.filter(
        (request) => request.chatId === JOURNEY_IDS.beta,
      ),
    ).toHaveLength(1);
    expectContinuityIdentityEvidence(result);
  });

  test("Project switch preserves the running owner and stale Project/Chat publication is rejected", async ({
    page,
  }, testInfo) => {
    await openJourney(page);
    const evidence = screenshotEvidence(testInfo, page);
    const alpha = pane(page, "Alpha");
    const composer = alpha.getByRole("textbox", { name: "Message agent" });
    await composer.fill("Keep this Turn locked");
    await alpha.getByRole("button", { name: "Send message" }).click();
    await runEvent(page, {
      type: "resolveSend",
      chat: "alpha",
      state: "sending",
    });
    await runEvent(page, { type: "publish", chat: "alpha", state: "running" });
    await evidence("project-a-running-owner");

    await page
      .getByRole("button", { name: /^Open chat Gamma(?:, pinned)?$/ })
      .click();
    await expectTurnState(page, alpha, 1, "running");
    await expect(alpha).toHaveClass(/agent-chat-pane--focused/);
    expect((await audit(page)).workbench).toMatchObject({
      projectId: JOURNEY_IDS.projectA,
      projectGeneration: 7,
      nativeTransitionEpoch: 7,
      activeTabId: JOURNEY_IDS.alpha,
    });
    await expect(pane(page, "Gamma")).toHaveCount(0);

    await focusPane(page, "Beta");
    await expectTurnState(page, alpha, 1, "running");
    await page
      .getByRole("button", { name: /^Open chat Gamma(?:, pinned)?$/ })
      .click();
    await expect(pane(page, "Gamma")).toBeVisible();
    await expect(
      page.getByRole("region", { name: "Agent chat Gamma" }),
    ).toHaveAttribute("data-project-id", JOURNEY_IDS.projectB);
    const acceptedGammaOwner = {
      projectId: JOURNEY_IDS.projectB,
      projectGeneration: 9,
      nativeTransitionEpoch: 9,
      activeTabId: JOURNEY_IDS.gamma,
      pinnedAgentChatOwner: JOURNEY_IDS.gamma,
      gammaScope: {
        projectId: JOURNEY_IDS.projectB,
        projectGeneration: "9",
        chatId: JOURNEY_IDS.gamma,
      },
      gammaProjection: {
        projectId: JOURNEY_IDS.projectB,
        chatId: JOURNEY_IDS.gamma,
        projectionRevision: "1",
      },
    };
    expect((await audit(page)).workbench).toMatchObject(acceptedGammaOwner);
    await evidence("project-b-active");

    await expect(
      runEvent(page, {
        type: "stalePublication",
        source: "alpha",
        target: "gamma",
      }),
    ).resolves.toBe("ignoredStaleIntent");
    await expect(
      page.getByRole("region", { name: "Agent chat Gamma" }),
    ).toHaveAttribute("data-chat-id", JOURNEY_IDS.gamma);
    await expect(
      page.getByRole("region", { name: "Agent chat Gamma" }),
    ).toHaveAttribute("data-project-id", JOURNEY_IDS.projectB);
    expect((await audit(page)).workbench).toMatchObject(acceptedGammaOwner);
    await evidence("stale-publication-rejected");

    await page
      .getByRole("button", { name: /^Open chat Alpha(?:, pinned)?$/ })
      .click();
    const restoredAlpha = pane(page, "Alpha");
    await expectTurnState(page, restoredAlpha, 1, "running");
    await expectRunningComposerReadyToQueue(restoredAlpha);
    await expect(
      restoredAlpha.getByRole("button", { name: "Cancel turn" }),
    ).toBeEnabled();
    expect((await audit(page)).workbench).toMatchObject({
      projectId: JOURNEY_IDS.projectA,
      projectGeneration: 11,
      nativeTransitionEpoch: 11,
      activeTabId: JOURNEY_IDS.alpha,
      pinnedAgentChatOwner: JOURNEY_IDS.alpha,
      alphaScope: {
        projectId: JOURNEY_IDS.projectA,
        projectGeneration: "11",
        chatId: JOURNEY_IDS.alpha,
      },
    });
    await evidence("project-a-running-owner-restored");

    const result = await audit(page);
    expect(result.eventCount).toBeLessThanOrEqual(MAX_SCRIPTED_EVENTS);
    expect(result.sendRequests).toHaveLength(1);
    expectStaleIdentityEvidence(result);
  });

  test("fixture admits 40 scripted events and rejects event 41", async ({
    page,
  }) => {
    await openJourney(page);
    await page.evaluate(async (limit) => {
      const fixture =
        await import("/tests/ui/fixtures/agentTurnJourneyFixture.ts");
      for (let index = 0; index < limit; index += 1) {
        await fixture.runJourneyEvent({ type: "noop" });
      }
    }, MAX_SCRIPTED_EVENTS);
    await expect(
      page.evaluate(async () => {
        const fixture =
          await import("/tests/ui/fixtures/agentTurnJourneyFixture.ts");
        return fixture.runJourneyEvent({ type: "noop" });
      }),
    ).rejects.toThrow("Scripted event limit 40 exceeded");
    const result = await audit(page);
    expect(result.eventCount).toBe(MAX_SCRIPTED_EVENTS);
    expect(result.eventHistory).toEqual(
      Array.from({ length: MAX_SCRIPTED_EVENTS }, () => "noop:noop"),
    );
  });
}
