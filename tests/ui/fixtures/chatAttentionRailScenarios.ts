import { expect, test, type Page } from "@playwright/test";
import { openJourney, settleRender } from "./agentTurnJourneyBrowser";

async function install(page: Page, reset = true) {
  await page.evaluate(async (fresh) => {
    const fixture =
      await import("/tests/ui/fixtures/chatAttentionJourneyFixture.ts");
    await fixture.installAttentionJourneyFixture(fresh);
  }, reset);
  await settleRender(page, true);
}

async function change(
  page: Page,
  chat: "alpha" | "beta" | "gamma",
  state: "waitingApproval" | "failed" | "completed",
) {
  await page.evaluate(
    async ({ chat, state }) => {
      const fixture =
        await import("/tests/ui/fixtures/chatAttentionJourneyFixture.ts");
      await fixture.publishAttentionJourney(chat, state);
    },
    { chat, state },
  );
  await settleRender(page, true);
}

async function audit(page: Page) {
  return page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/chatAttentionJourneyFixture.ts");
    return fixture.attentionJourneyAudit();
  });
}

export function registerAttentionRailScenarios() {
  test("T3-1 unopened approval failure and completion use summaries without transcript fetches", async ({
    page,
  }) => {
    await openJourney(page);
    await install(page);
    const beta = page
      .locator(".project-chat-row")
      .filter({ has: page.getByRole("button", { name: /^Open chat Beta/ }) });
    await change(page, "beta", "waitingApproval");
    await expect(
      beta.getByText("Awaiting approval", { exact: true }),
    ).toBeVisible();
    await change(page, "beta", "failed");
    await expect(beta.locator(".project-chat__state--failed")).toBeVisible();
    expect((await audit(page)).unread.beta).toBe(true);
    await change(page, "beta", "completed");
    expect((await audit(page)).unread.beta).toBe(false);
    await page.evaluate(async () => {
      const { chatAttentionStore } =
        await import("/src/app/chatAttentionStore.ts");
      await chatAttentionStore.markAllRead();
    });
    await change(page, "beta", "completed");
    await expect(beta.getByLabel("Unread updates")).toBeVisible();
    expect(
      (await audit(page)).fetches.filter((id) => id !== "journey-chat-alpha"),
    ).toEqual([]);
    expect((await audit(page)).transcriptSlots).toBeLessThanOrEqual(2);
  });

  test("T3-1 Settings and project switching acknowledge only foreground presented detail", async ({
    page,
  }) => {
    await openJourney(page);
    await install(page);
    await expect
      .poll(async () => (await audit(page)).rows.alpha.readPosition)
      .not.toBeNull();
    await page.evaluate(async () => {
      const { useAgentSettingsState } =
        await import("/src/features/settings/agentSettingsState.ts");
      useAgentSettingsState.getState().open();
    });
    await change(page, "alpha", "completed");
    const hiddenRead = (await audit(page)).rows.alpha.readPosition;
    await page.clock.runFor(250);
    expect((await audit(page)).rows.alpha.readPosition).toBe(hiddenRead);
    expect((await audit(page)).unread.alpha).toBe(true);
    await change(page, "gamma", "failed");
    expect((await audit(page)).rows.gamma.readPosition).toBeNull();
    await page.evaluate(async () => {
      const { useAgentSettingsState } =
        await import("/src/features/settings/agentSettingsState.ts");
      useAgentSettingsState.getState().close();
    });
    await settleRender(page, true);
    await expect.poll(async () => (await audit(page)).unread.alpha).toBe(false);
    await page.getByRole("button", { name: /^Open chat Gamma/ }).click();
    await expect(
      page.getByRole("region", { name: "Chat pane Gamma" }),
    ).toBeVisible();
    await settleRender(page, true);
    await expect.poll(async () => (await audit(page)).unread.gamma).toBe(false);
    expect((await audit(page)).rows.beta.readPosition).toBeNull();
  });

  test("T3-1 paired panes leave the unfocused pane unread and reload restores summary reads", async ({
    page,
  }) => {
    await openJourney(page);
    await install(page);
    await page.evaluate(async () => {
      const { useSideChatState } =
        await import("/src/workbench/sideChatState.ts");
      useSideChatState.setState({
        sides: [
          {
            id: "journey-chat-beta",
            projectId: "journey-project-a",
            parentChatId: "journey-chat-alpha",
          },
        ],
        pairedId: "journey-chat-beta",
      });
    });
    await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
    await change(page, "beta", "failed");
    expect((await audit(page)).rows.beta.readPosition).toBeNull();
    expect((await audit(page)).unread.beta).toBe(true);
    await page
      .getByRole("region", { name: "Chat pane Beta" })
      .getByRole("textbox", { name: "Message agent" })
      .focus();
    await settleRender(page, true);
    await expect.poll(async () => (await audit(page)).unread.beta).toBe(false);
    const read = (await audit(page)).rows.beta.readPosition;
    await page.reload();
    await expect(page.locator("main.workbench")).toBeVisible();
    await install(page, false);
    expect((await audit(page)).rows.beta.readPosition).toBe(read);
    expect((await audit(page)).unread.beta).toBe(false);
    expect((await audit(page)).transcriptSlots).toBeLessThanOrEqual(2);
  });
}
