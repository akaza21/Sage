import { expect, test, type Page } from "@playwright/test";
import { openScenario } from "./shared/visualBrowserPrimitives";
import {
  flowchartSource,
  pythonAnswer,
  tableAnswer,
  unlabelledAnswer,
} from "./fixtures/cp22Answers";

test.use({ deviceScaleFactor: 1 });

async function installAnswer(page: Page, text: string, streaming = false) {
  await page.evaluate(
    async ({ text, streaming }) => {
      const [{ buildVisualScenario }, { acpProjectionStoreFor }, ids] =
        await Promise.all([
          import("/tests/ui/fixtures/visualScenarioShell.ts"),
          import("/src/app/acpProjectionRegistry.ts"),
          import("/tests/ui/fixtures/visualFixtureIds.ts"),
        ]);
      const chatPage = buildVisualScenario("rich").pages[0];
      const turn = chatPage.turns[0];
      const item = turn.items.find(
        (candidate) =>
          candidate.kind === "text" && candidate.payload.role === "assistant",
      );
      if (!item || item.kind !== "text")
        throw new Error("Expected assistant text fixture");
      item.payload.text = text;
      item.lifecycle = streaming ? "streaming" : "completed";
      turn.state = streaming ? "running" : "completed";
      turn.items = [turn.items[0], item];
      acpProjectionStoreFor(chatPage.chatId).getState().applyAuthoritativePage(
        {
          projectId: ids.PROJECT_ID,
          projectGeneration: ids.GENERATION,
          chatId: chatPage.chatId,
        },
        chatPage,
      );
    },
    { text, streaming },
  );
  await page.locator(".acp-message--assistant").last().scrollIntoViewIfNeeded();
}

async function assertContained(page: Page) {
  const pane = page.locator(".agent-chat-pane").first();
  expect(
    await pane.evaluate(
      (element) => element.scrollWidth <= element.clientWidth + 1,
    ),
  ).toBe(true);
  const list = pane.locator(".message-list");
  const composer = pane.locator(".composer");
  const listBox = await list.boundingBox();
  const composerBox = await composer.boundingBox();
  expect(listBox && composerBox).toBeTruthy();
  if (listBox && composerBox)
    expect(listBox.y + listBox.height).toBeLessThanOrEqual(composerBox.y + 1);
}

for (const width of [1888, 560, 480]) {
  test(`CP-22 refs 28/56: tables, formatted lists and Python at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "rich", "agent");
    if (width !== 1888)
      await page.addStyleTag({
        content: `.agent-chat-panes { grid-template-columns: ${width}px !important; justify-content: center; }`,
      });
    await installAnswer(page, tableAnswer);
    await expect(page.getByRole("cell", { name: "a|b" })).toBeVisible();
    await expect(page.locator(".acp-markdown strong").first()).toHaveText(
      "Harnesses",
    );
    await assertContained(page);
    await expect(page.locator(".agent-chat-pane").first()).toHaveScreenshot(
      `cp22-table-${width}.png`,
    );
    await installAnswer(page, pythonAnswer);
    const code = page.getByRole("region", { name: "Python code" });
    await code.scrollIntoViewIfNeeded();
    await expect(code.locator("code span").first()).toBeVisible();
    await assertContained(page);
    await expect(page.locator(".agent-chat-pane").first()).toHaveScreenshot(
      `cp22-python-${width}.png`,
    );
  });

  test(`CP-22 ref 30: unlabelled hover, keyboard wrap and truthful copy at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "rich", "agent");
    if (width !== 1888)
      await page.addStyleTag({
        content: `.agent-chat-panes { grid-template-columns: ${width}px !important; justify-content: center; }`,
      });
    await installAnswer(page, unlabelledAnswer);
    const code = page.getByRole("region", { name: "Code block" });
    await code.hover();
    await expect(
      code.locator("header > span:not(.acp-code-block__actions)"),
    ).toHaveCount(0);
    expect(
      await code
        .locator("pre")
        .evaluate((element) => element.scrollWidth > element.clientWidth),
    ).toBe(true);
    await expect(code).toHaveScreenshot(`cp22-unlabelled-${width}.png`);
    const wrap = code.getByRole("button", { name: "Wrap code" });
    await wrap.focus();
    await page.keyboard.press("Enter");
    await expect(wrap).toHaveAttribute("aria-pressed", "true");
    expect(
      await code
        .locator("pre")
        .evaluate((element) => element.scrollWidth <= element.clientWidth + 1),
    ).toBe(true);
    await assertContained(page);
    await expect(code).toHaveScreenshot(`cp22-wrap-${width}.png`);
    await page.evaluate(() =>
      Object.defineProperty(navigator, "clipboard", {
        configurable: true,
        value: { writeText: async () => undefined },
      }),
    );
    await code.getByRole("button", { name: "Copy code" }).click();
    await expect(code.getByRole("status")).toHaveText("Copied");
    await expect(code).toHaveScreenshot(`cp22-copied-${width}.png`);
    await page.evaluate(() =>
      Object.defineProperty(navigator, "clipboard", {
        configurable: true,
        value: {
          writeText: async () => {
            throw new Error("Clipboard denied");
          },
        },
      }),
    );
    await code.getByRole("button", { name: "Copy code" }).click();
    await expect(code.getByRole("status")).toHaveText("Copy failed");
    await expect(code).toHaveScreenshot(`cp22-copy-failed-${width}.png`);
  });

  test(`CP-22 ref 55: routed flowchart, streaming and fallback at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "rich", "agent");
    if (width !== 1888)
      await page.addStyleTag({
        content: `.agent-chat-panes { grid-template-columns: ${width}px !important; justify-content: center; }`,
      });
    await installAnswer(
      page,
      `1. Password reset\n\n\`\`\`mermaid\n${flowchartSource}\n\`\`\``,
    );
    await expect(
      page.getByRole("img", { name: "Mermaid diagram" }),
    ).toBeVisible();
    await expect(page.locator("path[marker-end]")).toHaveCount(6);
    await assertContained(page);
    await expect(page.locator(".agent-chat-pane").first()).toHaveScreenshot(
      `cp22-flowchart-${width}.png`,
    );
    await installAnswer(page, "```mermaid\ngraph TD\nA -->", true);
    await expect(
      page.getByRole("status", { name: "Receiving diagram source" }),
    ).toBeVisible();
    await expect(page.locator(".acp-code-block")).toHaveScreenshot(
      `cp22-streaming-${width}.png`,
    );
    await installAnswer(page, "```mermaid\nunsupported diagram\n```");
    await expect(page.getByText(/could not be drawn/)).toBeVisible();
    await expect(page.locator(".acp-mermaid")).toHaveScreenshot(
      `cp22-fallback-${width}.png`,
    );
  });
}

test("CP-22 admitted image viewer remains keyboard accessible", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-image", "agent");
  const trigger = page
    .getByRole("button", { name: "Open image viewer" })
    .first();
  await trigger.scrollIntoViewIfNeeded();
  await trigger.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveScreenshot(
    "cp22-image-viewer.png",
  );
  await page.getByRole("button", { name: "Close image viewer" }).click();
  await expect(trigger).toBeFocused();
});

test("CP-22 Pi-compatible text-only projection has no fabricated image controls", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "rich", "agent");
  await installAnswer(
    page,
    `${tableAnswer}\n\n${pythonAnswer}\n\n\`\`\`mermaid\n${flowchartSource}\n\`\`\``,
  );
  await expect(
    page.getByRole("img", { name: "Mermaid diagram" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Open image viewer" }),
  ).toHaveCount(0);
  await expect(page.locator(".acp-image")).toHaveCount(0);
  await assertContained(page);
  await expect(page.locator(".agent-chat-pane").first()).toHaveScreenshot(
    "cp22-text-only.png",
  );
});
