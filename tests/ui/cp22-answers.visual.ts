import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";
import { revealWorkedContent } from "./agentVisualHelpers";
import { cp22MarkdownAnswers } from "./fixtures/cp22Answers";

test.use({ deviceScaleFactor: 1 });

async function expectFilledCodePanel(code: Locator) {
  const panel = await code.evaluate((element) => {
    const style = getComputedStyle(element);
    const swatch = document.createElement("span");
    swatch.style.color = "var(--color-surface-hover)";
    element.append(swatch);
    const fill = getComputedStyle(swatch).color;
    swatch.remove();
    return {
      fill,
      background: style.backgroundColor,
      radius: style.borderTopLeftRadius,
      radiusToken: style.getPropertyValue("--radius-xl").trim(),
      borders: [
        style.borderTopWidth,
        style.borderRightWidth,
        style.borderBottomWidth,
        style.borderLeftWidth,
      ],
    };
  });
  expect(panel.background).toBe(panel.fill);
  expect(panel.radius).toBe(panel.radiusToken);
  expect(panel.borders).toEqual(["0px", "0px", "0px", "0px"]);
}

async function expectDistinctActionFill(button: Locator) {
  const colors = await button.evaluate((element) => ({
    action: getComputedStyle(element).backgroundColor,
    panel: getComputedStyle(element.closest(".acp-code-block")!)
      .backgroundColor,
  }));
  expect(colors.action).not.toBe("rgba(0, 0, 0, 0)");
  expect(colors.action).not.toBe(colors.panel);
}

async function installAnswer(page: Page, text: string, streaming = false) {
  await settleChatProjections(page);
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
    await installAnswer(page, cp22MarkdownAnswers.table);
    await expect(page.getByRole("cell", { name: "a|b" })).toBeVisible();
    await expect(page.locator(".acp-markdown strong").first()).toHaveText(
      "Harnesses",
    );
    await settleTranscriptAtEnd(page);
    await assertContained(page);
    await expect(page.locator(".agent-chat-pane").first()).toHaveScreenshot(
      `cp22-table-${width}.png`,
    );
    await installAnswer(page, cp22MarkdownAnswers.python);
    const code = page.getByRole("region", { name: "Python code" });
    await code.scrollIntoViewIfNeeded();
    await expectFilledCodePanel(code);
    await expect(code.locator("code span").first()).toBeVisible();
    await settleTranscriptAtEnd(page);
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
    await installAnswer(page, cp22MarkdownAnswers.unlabelled);
    const code = page.getByRole("region", { name: "Code block" });
    await code.hover();
    await expectFilledCodePanel(code);
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
    await wrap.hover();
    await expectDistinctActionFill(wrap);
    await code.getByRole("button", { name: "Copy code" }).focus();
    await page.keyboard.press("Shift+Tab");
    await expect(wrap).toBeFocused();
    await page.mouse.move(0, 0);
    await expectDistinctActionFill(wrap);
    await page.keyboard.press("Enter");
    await expect(wrap).toHaveAttribute("aria-pressed", "true");
    await wrap.evaluate((element) => element.blur());
    await expectDistinctActionFill(wrap);
    await wrap.focus();
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
    await page.mouse.move(0, 0);
    await expectDistinctActionFill(
      code.getByRole("button", { name: "Copy code" }),
    );
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
    await page.mouse.move(0, 0);
    await code
      .getByRole("button", { name: "Copy code" })
      .evaluate((element) => element.blur());
    await expectDistinctActionFill(
      code.getByRole("button", { name: "Copy code" }),
    );
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
    await installAnswer(page, cp22MarkdownAnswers.passwordReset);
    await expect(
      page.getByRole("img", { name: "Mermaid diagram" }),
    ).toBeVisible();
    await expect(page.locator("path[marker-end]")).toHaveCount(6);
    await settleTranscriptAtEnd(page);
    await assertContained(page);
    await expect(page.locator(".agent-chat-pane").first()).toHaveScreenshot(
      `cp22-flowchart-${width}.png`,
    );
    await installAnswer(page, cp22MarkdownAnswers.streaming, true);
    await expect(
      page.getByRole("status", { name: "Receiving diagram source" }),
    ).toBeVisible();
    await expect(page.locator(".acp-code-block")).toHaveScreenshot(
      `cp22-streaming-${width}.png`,
    );
    await installAnswer(page, cp22MarkdownAnswers.fallback);
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
  await revealWorkedContent(page.locator(".acp-image"));
  const trigger = page
    .getByRole("button", { name: "Open image viewer" })
    .first();
  await expect(trigger).toBeVisible();
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

test("CP-22 numbered setup answer keeps commands inside their step", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "rich", "agent");
  await installAnswer(
    page,
    "## Run the app\n\n1. Run this command\n\n   ```sh\n   npm install\n   ```\n\n   Wait for the dependencies to finish installing.\n\n2. Start the app\n\n   ```sh\n   npm run dev\n   ```\n\n   Open the local address printed in the terminal.",
  );
  const answer = page
    .locator(".acp-message--assistant")
    .last()
    .locator(".acp-markdown");
  const steps = answer.locator("ol > li");
  await expect(steps).toHaveCount(2);
  await expect(steps.nth(0).locator("pre")).toHaveText("npm install");
  await expect(steps.nth(1).locator("pre")).toHaveText("npm run dev");
  expect(
    await steps
      .nth(0)
      .locator("pre")
      .evaluate((command) =>
        Boolean(
          command.compareDocumentPosition(
            command.closest("li")!.nextElementSibling!,
          ) & Node.DOCUMENT_POSITION_FOLLOWING,
        ),
      ),
  ).toBe(true);
  await expect(
    steps.nth(0).getByRole("button", { name: "Copy code" }),
  ).toBeVisible();
  await settleTranscriptAtEnd(page);
  await assertContained(page);
  await expect(answer).toHaveScreenshot("cp22-numbered-commands.png");
});

test("CP-22 quoted blocks keep code whitespace and their real renderers", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "rich", "agent");
  await installAnswer(
    page,
    "> ## Quoted heading\n>\n> ```python\n> def greet(name):\n>     if name:\n>         print(name)\n>\n>     return name\n> ```\n>\n> | Name | Value |\n> | --- | --- |\n> | key | **value** |\n>\n> ```mermaid\n> flowchart LR\n> A --> B\n> ```\n>\n> - Child\n>   > Nested quote",
  );
  const quote = page
    .locator(".acp-message--assistant")
    .last()
    .locator(".acp-markdown > blockquote");
  await expect(
    quote.getByRole("heading", { name: "Quoted heading" }),
  ).toBeVisible();
  const code = quote.getByRole("region", { name: "Python code" });
  expect(await code.locator("pre").textContent()).toBe(
    "def greet(name):\n    if name:\n        print(name)\n\n    return name",
  );
  await expect(code.getByRole("button", { name: "Copy code" })).toBeVisible();
  await expect(quote.getByRole("table").locator("td strong")).toHaveText(
    "value",
  );
  await expect(
    quote.getByRole("img", { name: "Mermaid diagram" }),
  ).toBeVisible();
  await expect(quote.locator("li blockquote")).toHaveText("Nested quote");
  expect(
    await quote.evaluate((element) =>
      [...element.children].map((child) => child.tagName),
    ),
  ).toEqual(["H2", "DIV", "DIV", "DIV", "UL"]);
});

test("CP-22 Pi-compatible text-only projection has no fabricated image controls", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "rich", "agent");
  await installAnswer(page, cp22MarkdownAnswers.combined);
  await expect(
    page.getByRole("img", { name: "Mermaid diagram" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Open image viewer" }),
  ).toHaveCount(0);
  await expect(page.locator(".acp-image")).toHaveCount(0);
  await settleTranscriptAtEnd(page);
  await assertContained(page);
  await expect(page.locator(".agent-chat-pane").first()).toHaveScreenshot(
    "cp22-text-only.png",
  );
});
