import { expect, test } from "./shared/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

test.use({ deviceScaleFactor: 1 });

for (const harness of ["claude-code", "codex", "pi"]) {
  test(`${harness}: one status, reported retry and terminal causes`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    const install = async (
      mode: "approval" | "retry" | "failed" | "denied" | "cancelled" | "edits",
    ) => {
      await page.evaluate(
        async ({ mode, harness }) => {
          const fixture = await sageImport(
            "/tests/ui/fixtures/chatSurfaceFixture.ts",
          );
          fixture.installChatSurfaceQaFixture(mode, harness);
        },
        { mode, harness },
      );
    };
    await install("approval");
    await expect(
      page.locator(".agent-task-header").getByRole("status"),
    ).toHaveText("Awaiting approval");
    await expect(
      page
        .locator(".project-chat-row")
        .filter({ hasText: "Inspect repo files and math.js" })
        .first(),
    ).toContainText("Awaiting approval");
    await install("retry");
    await expect(
      page.getByText("Attempt failed (attempt 1/3)", { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText("Server unavailable", { exact: true }),
    ).toBeVisible();
    await install("failed");
    await expect(
      page.getByRole("group", { name: "Turn failed" }),
    ).toContainText("Rate limited by provider");
    await install("denied");
    await expect(page.locator("[data-turn-reason-message]")).toHaveText(
      "Write blocked: read-only access",
    );
    await install("cancelled");
    const retry = page.getByRole("button", { name: "Retry", exact: true });
    await expect(retry).toBeVisible();
    expect((await retry.boundingBox())!.width).toBeLessThan(100);
    await install("edits");
    const summary = page.getByRole("region", {
      name: "Files edited in this chat",
    });
    await expect(summary).toHaveCount(1);
    await expect(
      page.getByRole("region", { name: "Edited files", exact: true }),
    ).toHaveCount(0);
    await expect(summary.getByLabel("Line changes")).toHaveText("+7-2");
  });
}

test("Copy confirms success and transcript shares the composer measure across widths", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const fixture = await sageImport(
      "/tests/ui/fixtures/chatSurfaceFixture.ts",
    );
    fixture.installChatSurfaceFixture();
  });
  const actions = page.getByRole("group", { name: "Message actions" });
  await actions.getByRole("button", { name: "Copy", exact: true }).click();
  await expect(
    actions.getByRole("button", { name: "Copied", exact: true }),
  ).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toContain(
    "Inspected all four repository files",
  );
  await expect(
    actions.getByRole("button", { name: "Copy", exact: true }),
  ).toBeVisible();
  for (const width of [1280, 1872, 800]) {
    await page.setViewportSize({ width, height: 800 });
    const content = (await page
      .locator(".message-list__content")
      .boundingBox())!;
    const dock = (await page.locator(".chat-dock").boundingBox())!;
    expect(Math.abs(content.width - dock.width)).toBeLessThanOrEqual(1);
    expect(Math.abs(content.x - dock.x)).toBeLessThanOrEqual(1);
    if (width === 1872) expect(content.width).toBeGreaterThan(660);
  }
  await page
    .getByRole("radio", { name: "Editor", exact: true })
    .locator("..")
    .click();
  // Workbench's tested Activity contract suspends the retained Agent body.
  await expect(page.locator(".acp-chat-projection")).toBeHidden();
  await page
    .getByRole("radio", { name: "Agent", exact: true })
    .locator("..")
    .click();
  await expect(page.locator(".acp-chat-projection")).toBeVisible();
});
