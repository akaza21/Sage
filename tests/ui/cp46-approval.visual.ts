import { expect, test, type Page } from "@playwright/test";

import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

/** A running command gated by a pending Codex-shaped approval. */
async function applyApproval(page: Page, state: "received" | "declinePending") {
  await page.evaluate(async (state) => {
    const [{ buildVisualScenario }, { acpProjectionStoreFor }, ids] =
      await Promise.all([
        import("/tests/ui/fixtures/visualScenarioShell.ts"),
        import("/src/app/acpProjectionRegistry.ts"),
        import("/tests/ui/fixtures/visualFixtureIds.ts"),
      ]);
    const scenario = buildVisualScenario("waiting");
    const chatPage = scenario.pages[0];
    const turn = chatPage.turns[0];
    const prompt = turn.items[0];
    turn.state = "running";
    turn.terminalAt = null;
    turn.items.push(
      {
        ...prompt,
        id: "approval-command" as typeof prompt.id,
        ordinal: 4,
        kind: "toolCall",
        lifecycle: "streaming",
        toolState: "inProgress",
        terminalAt: null,
        payload: {
          presentation: {
            kind: "execute",
            label: "curl.exe",
            summary: null,
            cwd: null,
            exitCode: null,
            outputTail: null,
            outputTruncated: false,
            resources: [],
            activity: "ordinary",
          },
          inputSummary:
            'curl.exe -s -o NUL -w "%{http_code}" https://example.com',
        },
      } as never,
      {
        ...prompt,
        id: "approval-request" as typeof prompt.id,
        ordinal: 5,
        kind: "permissionRequest",
        lifecycle: "streaming",
        toolState: "waitingForApproval",
        terminalAt: null,
        payload: {
          requestId: "approval-1",
          state,
          label: "Permission request",
          summary: "Run a command",
          source: "harness",
          offeredOptionKinds: ["allow_once", "allow_always", "reject_once"],
          category: "execute",
          reason:
            "May I retry the network check outside the sandbox? The sandboxed request returned 000 without an HTTP response.",
        },
      } as never,
    );
    acpProjectionStoreFor(chatPage.chatId).getState().applyAuthoritativePage(
      {
        projectId: ids.PROJECT_ID,
        projectGeneration: ids.GENERATION,
        chatId: chatPage.chatId,
      },
      chatPage,
    );
  }, state);
}

test("CP-46 approval replaces the composer slot, opens its menu above, and yields back", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await openScenario(page, "waiting", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer");
  const before = await composer.boundingBox();
  await applyApproval(page, "received");

  // The composer-slot card; once settled, the transcript keeps its own
  // compact "Permission request" record (Codex s6/f0569 keeps the turn).
  const prompt = page
    .locator(".acp-permission-prompt")
    .and(page.getByRole("group", { name: "Permission request" }));
  await expect(prompt).toBeVisible();
  await expect(composer).toBeHidden();
  await expect(prompt.getByRole("button", { name: /Deny/ })).toBeVisible();
  const allow = prompt.getByRole("button", { name: /Allow once/ });
  await expect(allow).toBeVisible();
  await expect(allow).toBeFocused();
  // The card shows the request's own text; the live command row is not
  // linked to the request on the wire, so it is never guessed.
  await expect(prompt.locator("pre")).toHaveText("Run a command");
  await expect(prompt.locator(".acp-permission-prompt__question")).toHaveText(
    "Allow this command?",
  );
  await expect(prompt.locator(".acp-permission-prompt__reason")).toContainText(
    "Agent's reason:",
  );
  // Stop stays for safety, as a quiet header control (s6/f0526 has none).
  const stop = prompt.getByRole("button", { name: "Stop turn" });
  await expect(stop).toBeVisible();
  await expect(prompt.locator("header")).toContainText("Stop");
  await expect(stop).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  // Allow once holds focus on arrival and shows Codex's 2 px ring: a 1 px
  // accent outline plus a 1 px accent spread (no 2 px outlines by policy).
  await expect(allow).toHaveCSS("outline-style", "solid");
  await expect(allow).toHaveCSS("outline-color", "rgb(58, 131, 247)");
  await expect(allow).toHaveCSS(
    "box-shadow",
    "rgb(58, 131, 247) 0px 0px 0px 1px",
  );
  // The running row keeps a spinner beside the Awaiting approval pill.
  await expect(
    page.locator(".project-chat__approval-spinner").first(),
  ).toBeVisible();
  const box = await prompt.boundingBox();
  expect(Math.abs(box!.width - before!.width)).toBeLessThanOrEqual(2);
  expect(Math.abs(box!.x - before!.x)).toBeLessThanOrEqual(2);

  const chevron = prompt.getByRole("button", { name: "More allow options" });
  await chevron.click();
  const menu = page.getByRole("menu", { name: "Allow options" });
  await expect(menu.getByRole("menuitem")).toHaveText([
    "Allow once",
    "Allow similar commands",
  ]);
  const menuBox = await menu.boundingBox();
  const chevronBox = await chevron.boundingBox();
  expect(menuBox!.y + menuBox!.height).toBeLessThanOrEqual(chevronBox!.y);
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(prompt).toBeVisible();

  const pill = page.getByText("Awaiting approval", { exact: true });
  await expect(pill.first()).toBeVisible();
  await expect(pill.first()).toHaveCSS("color", "rgb(64, 201, 119)");
  await expect(pill.first()).toHaveCSS("background-color", "rgb(49, 76, 60)");

  await applyApproval(page, "declinePending");
  await expect(prompt).toHaveCount(0);
  await expect(composer).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "Message agent" }),
  ).toBeVisible();
  await expect(
    page.getByText("Awaiting approval", { exact: true }),
  ).toHaveCount(0);
});

for (const width of [720, 480]) {
  test(`CP-46 approval card fills the composer slot at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await openScenario(page, "waiting", "agent");
    await settleChatProjections(page);
    const composer = page.locator("form.composer");
    const before = (await composer.boundingBox())!;
    await applyApproval(page, "received");
    const prompt = page
      .locator(".acp-permission-prompt")
      .and(page.getByRole("group", { name: "Permission request" }));
    await expect(prompt).toBeVisible();
    const box = (await prompt.boundingBox())!;
    expect(Math.abs(box.width - before.width)).toBeLessThanOrEqual(2);
    expect(Math.abs(box.x - before.x)).toBeLessThanOrEqual(2);
    // Deny and Allow keep to the right edge, wrapped or not.
    const actions = (await prompt
      .locator(".acp-permission-prompt__actions")
      .boundingBox())!;
    for (const button of [prompt.locator(".acp-permission-prompt__split")]) {
      const edge = (await button.boundingBox())!;
      expect(
        Math.abs(edge.x + edge.width - (actions.x + actions.width)),
      ).toBeLessThanOrEqual(1);
    }
    const deny = (await prompt
      .getByRole("button", { name: /Deny/ })
      .boundingBox())!;
    const split = (await prompt
      .locator(".acp-permission-prompt__split")
      .boundingBox())!;
    if (deny.y + 1 < split.y)
      expect(
        Math.abs(deny.x + deny.width - (actions.x + actions.width)),
      ).toBeLessThanOrEqual(1);
    expect(box.x + box.width).toBeLessThanOrEqual(width);
  });
}
