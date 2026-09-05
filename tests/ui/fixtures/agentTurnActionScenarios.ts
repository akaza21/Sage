import { expect, test } from "@playwright/test";

import {
  audit,
  expectRequestCount,
  focusPane,
  JOURNEY_IDS,
  openJourney,
  pane,
  runEvent,
  screenshotEvidence,
  settleRender,
  turnStatus,
} from "./agentTurnJourneyBrowser";
import {
  expectActionIdentityEvidence,
  expectRecoveryIdentityEvidence,
} from "./agentTurnJourneyEvidence";
import { MAX_SCRIPTED_EVENTS } from "./agentTurnJourneyLimits";

export function registerAgentTurnActionScenarios() {
  test("same-Chat duplicate is visibly rejected while a second Chat sends and cancellation converges", async ({
    page,
  }, testInfo) => {
    await openJourney(page);
    const evidence = screenshotEvidence(testInfo, page);
    const alpha = pane(page, "Alpha");
    const alphaComposer = alpha.getByRole("textbox", { name: "Message agent" });
    const alphaSend = alpha.getByRole("button", { name: "Send message" });
    const beta = await focusPane(page, "Beta");
    const betaComposer = beta.getByRole("textbox", { name: "Message agent" });
    await betaComposer.fill("Preserve this Beta draft");
    await focusPane(page, "Alpha");
    await expect(betaComposer).toHaveValue("Preserve this Beta draft");

    await alphaComposer.fill("Run Alpha once");
    await alphaSend.click();
    await expect(alphaComposer).toHaveValue("");
    await expect(alphaComposer).toBeDisabled();
    await expect(
      alpha.getByText("Wait for the current action to finish.", {
        exact: true,
      }),
    ).toBeVisible();
    await expect(betaComposer).toHaveValue("Preserve this Beta draft");
    await evidence("alpha-send-pending");

    await alphaSend.click({ force: true });
    await expectRequestCount(page, "sendRequests", 1);
    await expect(
      alpha.getByText("Wait for the current action to finish.", {
        exact: true,
      }),
    ).toBeVisible();

    await runEvent(page, {
      type: "resolveSend",
      chat: "alpha",
      state: "sending",
    });
    await expect(alpha.getByRole("status", { name: "Sending" })).toBeVisible();
    await expect(betaComposer).toHaveValue("Preserve this Beta draft");
    await evidence("alpha-sending");

    await runEvent(page, { type: "publish", chat: "alpha", state: "running" });
    await expect(alpha.getByRole("status", { name: "Running" })).toBeVisible();
    await expect(alphaComposer).toBeDisabled();
    await expect(
      alpha.getByText(
        "Wait for the active Turn to finish, or cancel it when available.",
        { exact: true },
      ),
    ).toBeVisible();
    await expect(betaComposer).toHaveValue("Preserve this Beta draft");
    await evidence("alpha-running");

    await focusPane(page, "Beta");
    expect((await audit(page)).workbench).toMatchObject({
      projectId: JOURNEY_IDS.projectA,
      activeTabId: JOURNEY_IDS.beta,
      projectTransitioning: false,
      selectedHarnessId: "journey-acp",
      selectedModelId: "journey-model",
      models: [{ id: "journey-model" }],
      harnessDiscoveryPhase: "ready",
      betaProjectionChatId: JOURNEY_IDS.beta,
    });
    await expect(betaComposer).toHaveValue("Preserve this Beta draft");
    await betaComposer.fill("Run Beta independently");
    await beta.getByRole("button", { name: "Send message" }).click();
    await expect(betaComposer).toBeDisabled();
    await expectRequestCount(page, "sendRequests", 2);
    await runEvent(page, {
      type: "resolveSend",
      chat: "beta",
      state: "sending",
    });
    await runEvent(page, { type: "publish", chat: "beta", state: "running" });
    await expect(beta.getByRole("status", { name: "Running" })).toBeVisible();
    await expect(alpha.getByRole("status", { name: "Running" })).toBeVisible();
    await evidence("independent-beta-running");

    await focusPane(page, "Alpha");
    await expect(alphaComposer).toBeDisabled();
    await alpha.getByRole("button", { name: "Cancel turn" }).click();
    await expect(
      alpha.getByRole("button", { name: "Cancel turn" }),
    ).toBeDisabled();
    await expectRequestCount(page, "cancelRequests", 1);
    await evidence("alpha-cancel-pending");
    await runEvent(page, { type: "resolveCancel", chat: "alpha" });
    await expect(
      alpha.getByRole("status", { name: "Cancelling" }),
    ).toBeVisible();
    await evidence("alpha-cancelling");
    await runEvent(page, {
      type: "publish",
      chat: "alpha",
      state: "cancelled",
    });
    await expect(turnStatus(alpha, 1, "Cancelled")).toBeVisible();
    await expect(alphaComposer).toBeEnabled();
    await expect(beta.getByRole("status", { name: "Running" })).toBeVisible();
    await evidence("alpha-cancelled-beta-unaffected");

    const result = await audit(page);
    expect(result.eventCount).toBeLessThanOrEqual(MAX_SCRIPTED_EVENTS);
    expect(result.pendingCalls).toBe(0);
    expect(result.sendRequests).toHaveLength(2);
    expect(result.cancelRequests).toHaveLength(1);
    expectActionIdentityEvidence(result);
  });

  test("definite non-dispatch restores once while deliveryUnknown never restores or replays", async ({
    page,
  }, testInfo) => {
    await openJourney(page);
    const evidence = screenshotEvidence(testInfo, page);
    const alpha = pane(page, "Alpha");
    const composer = alpha.getByRole("textbox", { name: "Message agent" });

    await composer.fill("Restore this exact prompt");
    await alpha.getByRole("button", { name: "Send message" }).click();
    await expect(composer).toHaveValue("");
    await runEvent(page, {
      type: "rejectSend",
      chat: "alpha",
      certainty: "definite",
    });
    await expect(alpha.getByRole("alert")).toHaveText(
      "The prompt was definitely not dispatched.",
    );
    await expect(composer).toHaveValue("Restore this exact prompt");
    await evidence("definite-nondispatch-restored");

    await settleRender(page, true);
    const afterFailure = await audit(page);
    const alphaHistory = afterFailure.composerHistory.filter(
      (entry) => entry.chatId === JOURNEY_IDS.alpha,
    );
    expect(alphaHistory.slice(-3)).toEqual([
      { chatId: JOURNEY_IDS.alpha, text: "Restore this exact prompt" },
      { chatId: JOURNEY_IDS.alpha, text: "" },
      { chatId: JOURNEY_IDS.alpha, text: "Restore this exact prompt" },
    ]);

    await composer.fill("Uncertain delivery prompt");
    await alpha.getByRole("button", { name: "Send message" }).click();
    await runEvent(page, {
      type: "resolveSend",
      chat: "alpha",
      state: "deliveryUnknown",
    });
    await expect(turnStatus(alpha, 1, "Delivery unknown")).toBeVisible();
    await expect(turnStatus(alpha, 1, "Delivery unknown")).toHaveText(
      "Delivery unknown",
    );
    await expect(
      alpha.getByRole("button", { name: "Cancel turn" }),
    ).toHaveCount(0);
    await expect(composer).toBeEnabled();
    await expect(composer).toHaveValue("");
    await evidence("delivery-unknown-attention");

    await page.clock.runFor(500);
    await settleRender(page);
    const result = await audit(page);
    expect(result.sendRequests).toHaveLength(2);
    expect(result.sendRequests[1]).toMatchObject({
      chatId: JOURNEY_IDS.alpha,
      sessionId: "journey-session-alpha",
      runtimeEpoch: "11",
      prompt: "Uncertain delivery prompt",
    });
    expect(result.composerHistory.at(-1)).toEqual({
      chatId: JOURNEY_IDS.alpha,
      text: "",
    });
    expect(result.eventCount).toBeLessThanOrEqual(MAX_SCRIPTED_EVENTS);
    expectRecoveryIdentityEvidence(result);
  });
}
