import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";

test.use({ deviceScaleFactor: 1 });

async function box(locator: Locator) {
  const bounds = await locator.boundingBox();
  expect(bounds).not.toBeNull();
  return bounds!;
}

async function collapseArtifactPane(page: Page) {
  // The diff scenario may open with the artifact pane already collapsed.
  const collapse = page.getByRole("button", {
    name: "Collapse files, changes, and preview",
  });
  const expand = page.getByRole("button", { name: /^Show files and preview/ });
  await expect(collapse.or(expand).first()).toBeVisible();
  if (await collapse.isVisible()) await collapse.click();
  await expect(expand).toBeVisible();
}

async function edit(
  page: Page,
  lifecycle: "streaming" | "completed" | "failed",
  chatId?: string,
) {
  await page.evaluate(
    async ({ state, chatId }) => {
      const fixture = await import("/tests/ui/fixtures/cp30cEditFixture.ts");
      fixture.installCp30cEdit(state, chatId);
    },
    { state: lifecycle, chatId },
  );
  await settleTranscriptAtEnd(page);
}

for (const width of [1888, 720, 480]) {
  test(
    "CP-30c live row, centred pill and visible filenames at " + width,
    async ({ page }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, "diff", "agent");
      await settleChatProjections(page);
      await collapseArtifactPane(page);
      await edit(page, "streaming");
      const pane = page.locator(".agent-chat-pane");
      const pill = pane.getByRole("button", { name: /^1 file changed/ });
      const summary = pane.getByRole("region", {
        name: "Files edited in this chat",
      });
      await expect(pane.locator(".acp-editing-files")).toBeVisible();
      await expect(pill).toHaveAttribute("aria-expanded", "false");
      await expect(summary.locator("li")).toHaveCount(0);
      await expect(pane.locator(".message-list .acp-file-changes")).toHaveCount(
        0,
      );
      const column = await box(pane.locator(".acp-turn__items").last());
      const bounds = await box(summary);
      const composer = await box(pane.locator("form.composer"));
      expect(
        Math.abs(bounds.x + bounds.width / 2 - column.x - column.width / 2),
      ).toBeLessThanOrEqual(1);
      expect(bounds.x).toBeGreaterThanOrEqual(column.x);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(
        column.x + column.width,
      );
      expect(bounds.y + bounds.height).toBeLessThanOrEqual(composer.y);
      expect(
        Math.abs(bounds.x + bounds.width / 2 - composer.x - composer.width / 2),
      ).toBeLessThanOrEqual(1);
      await pill.click();
      const filename = summary.locator(".chat-file-chip__name").first();
      await expect(filename).toHaveText("AgentChatPanes.tsx");
      await expect(filename).toBeVisible();
      const name = await box(filename);
      expect(name.width).toBeGreaterThanOrEqual(48);
      const disclosure = await box(summary);
      expect(Math.abs(disclosure.x - composer.x)).toBeLessThanOrEqual(1);
      expect(Math.abs(disclosure.width - composer.width)).toBeLessThanOrEqual(
        1,
      );
      expect(name.x).toBeGreaterThanOrEqual(disclosure.x);
      expect(name.x + name.width).toBeLessThanOrEqual(
        disclosure.x + disclosure.width,
      );
      await summary
        .getByRole("button", {
          name: "Open diff Sage / src/workbench/AgentChatPanes.tsx",
        })
        .click();
      await expect(
        page.getByRole("table", { name: "Unified file diff" }),
      ).toBeVisible();
      await expect(
        page.getByRole("tab", { name: "Review", exact: true }),
      ).toHaveCount(0);
      await collapseArtifactPane(page);
      await pill.click();
      await edit(page, "completed");
      await expect(pane.locator(".acp-editing-files")).toHaveCount(0);
      await expect(
        pane.getByRole("button", { name: /^2 files changed/ }),
      ).toHaveAttribute("aria-expanded", "false");
      await expect(pane.locator('[data-turn-state="running"]')).toHaveCount(1);
      await edit(page, "failed");
      await expect(pane.locator(".acp-editing-files")).toHaveCount(0);
    },
  );
}

for (const width of [1888, 720, 480]) {
  test(
    "CP-30c queue and files pill remain hit-testable at " + width,
    async ({ page }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, "agent-single", "agent");
      await settleChatProjections(page);
      await collapseArtifactPane(page);
      await page.evaluate(async () => {
        const fixture =
          await import("/tests/ui/fixtures/chatContainmentFixture.ts");
        fixture.installChatContainmentFixture(
          {
            name: "Codex",
            effort: true,
            context: true,
            goal: true,
            steer: true,
          },
          true,
        );
      });
      const pane = page.locator(".agent-chat-pane");
      const composer = pane.locator("form.composer");
      const summary = pane.getByRole("region", {
        name: "Files edited in this chat",
      });
      const pill = summary.getByRole("button", { name: /files? changed/ });
      const queue = composer.locator(".composer__queued");
      await expect(queue).toBeVisible();
      await expect(pill).toBeVisible();
      const queueBounds = await box(queue);
      const pillBounds = await box(pill);
      expect(pillBounds.y + pillBounds.height).toBeLessThanOrEqual(
        queueBounds.y,
      );
      expect(queueBounds.height).toBeLessThanOrEqual(320);
      await composer
        .getByRole("button", { name: /^Delete queued message:/ })
        .click({ trial: true });
      await pill.click();
      await expect(pill).toHaveAttribute("aria-expanded", "true");
      const expanded = await box(summary);
      const column = await box(composer);
      expect(Math.abs(expanded.x - column.x)).toBeLessThanOrEqual(1);
      expect(Math.abs(expanded.width - column.width)).toBeLessThanOrEqual(1);
      const summarySlot = await box(pane.locator(".chat-edit-summary-zone"));
      expect(summarySlot.y + summarySlot.height).toBeLessThanOrEqual(
        (await box(queue)).y,
      );
      await summary
        .getByRole("button", { name: /^Open diff Review / })
        .last()
        .click({ trial: true });
      await composer
        .getByRole("button", { name: /^Delete queued message:/ })
        .click({ trial: true });
      await pill.click();
    },
  );
}

for (const width of [720, 480]) {
  test(
    "CP-30c expanded disclosure preserves composer and transcript at " +
      width +
      "x560",
    async ({ page }) => {
      await page.setViewportSize({ width, height: 560 });
      await openScenario(page, "diff", "agent");
      await settleChatProjections(page);
      await collapseArtifactPane(page);
      const pane = page.locator(".agent-chat-pane");
      const summary = pane.getByRole("region", {
        name: "Files edited in this chat",
      });
      for (const totals of ["unknown", "partial"] as const) {
        await page.evaluate(async (totals) => {
          const fixture =
            await import("/tests/ui/fixtures/cp30cEditFixture.ts");
          fixture.installCp30cDisclosure(totals);
        }, totals);
        const pill = summary.getByRole("button", { name: /files changed/ });
        if ((await pill.getAttribute("aria-expanded")) === "false")
          await pill.click();
        await expect(pill).toHaveAttribute("aria-expanded", "true");
        const header = summary.locator(".acp-file-changes__header");
        await expect(
          header.locator(".acp-file-changes__additions"),
        ).toHaveCount(0);
        if (totals === "unknown") {
          await expect(header.locator(".acp-file-changes__totals")).toHaveCount(
            0,
          );
        } else {
          await expect(
            header.locator(".acp-file-changes__deletions"),
          ).toHaveText(/^-\d+$/);
        }
        const archived = summary
          .locator("li")
          .filter({ has: page.getByText("archived.ts", { exact: true }) });
        await expect(archived.getByRole("button")).toHaveCount(0);
        const current = summary
          .locator("li")
          .filter({ has: page.getByText("disclosure-0.tsx", { exact: true }) });
        await expect(
          current.getByRole("button", {
            name: "Open diff Sage / src/disclosure-0.tsx",
            exact: true,
          }),
        ).toBeEnabled();
        const bounds = await box(summary);
        const surface = await box(pane.locator(".chat-surface"));
        const transcript = await box(pane.locator(".message-list"));
        const composer = await box(pane.locator("form.composer"));
        const column = await box(pane.locator(".message-list__content"));
        expect(transcript.height).toBeGreaterThanOrEqual(120);
        expect(composer.y).toBeGreaterThanOrEqual(surface.y + 120);
        expect(transcript.y + transcript.height).toBeLessThanOrEqual(
          bounds.y + 1,
        );
        const summarySlot = await box(pane.locator(".chat-edit-summary-zone"));
        expect(summarySlot.y + summarySlot.height).toBeLessThanOrEqual(
          composer.y + 1,
        );
        expect(composer.y + composer.height).toBeLessThanOrEqual(
          surface.y + surface.height + 1,
        );
        expect(composer.y + composer.height).toBeLessThanOrEqual(560);
        const toolbar = await box(pane.locator(".composer__toolbar"));
        expect(toolbar.y).toBeGreaterThanOrEqual(composer.y);
        expect(toolbar.y + toolbar.height).toBeLessThanOrEqual(
          composer.y + composer.height,
        );
        expect(
          Math.abs(bounds.x + bounds.width / 2 - column.x - column.width / 2),
        ).toBeLessThanOrEqual(1);
        expect(bounds.x).toBeGreaterThanOrEqual(column.x);
        expect(bounds.x + bounds.width).toBeLessThanOrEqual(
          column.x + column.width,
        );
        for (const filename of await summary
          .locator(".chat-file-chip__name")
          .all()) {
          const name = await box(filename);
          expect(name.width).toBeGreaterThanOrEqual(48);
        }
        expect(
          await pane
            .locator(".chat-edit-summary-zone")
            .evaluate(
              (zone) =>
                zone.scrollHeight > zone.clientHeight ||
                Array.from(zone.children).some(
                  (child) => child.scrollHeight > child.clientHeight,
                ),
            ),
        ).toBe(true);
      }
    },
  );
}

test("CP-30c switching open chats keeps disclosure and activity isolated", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-dual", "agent");
  await settleChatProjections(page);
  const panes = page.locator(".agent-chat-pane");
  await expect(panes).toHaveCount(1);
  const first = panes.first();
  const owner = await first
    .locator(".acp-chat-projection")
    .getAttribute("data-chat-id");
  expect(owner).not.toBeNull();
  await edit(page, "streaming", owner!);
  await expect(first.locator(".acp-editing-files")).toBeVisible();
  await first.getByRole("button", { name: /^2 files changed/ }).click();
  await expect(first.locator(".acp-file-changes li")).toHaveCount(2);
  await page
    .getByRole("button", { name: /^Open chat Editor consistency pass/ })
    .click();
  await expect(panes).toHaveCount(1);
  await expect(panes.first().locator(".acp-file-changes")).toHaveCount(0);
  await expect(panes.first().locator(".acp-editing-files")).toHaveCount(0);
  await edit(page, "completed", owner!);
  await page
    .getByRole("button", { name: /^Open chat Agent navigation polish/ })
    .click();
  await expect(first.locator(".acp-editing-files")).toHaveCount(0);
});

test("CP-30d the selected chat keeps edit zones and queued trays free of resize loops", async ({
  page,
}) => {
  // Chromium reports ResizeObserver loops as a window error event without an
  // Error object, so collect them in-page as well as from the console.
  await page.addInitScript(() => {
    const target = window as unknown as { __resizeLoops: string[] };
    target.__resizeLoops = [];
    window.addEventListener(
      "error",
      (event) => {
        if (String(event.message).includes("ResizeObserver loop"))
          target.__resizeLoops.push(String(event.message));
      },
      true,
    );
  });
  const resizeErrors: string[] = [];
  const recordResizeError = (message: string) => {
    if (message.includes("ResizeObserver loop")) resizeErrors.push(message);
  };
  page.on("pageerror", (error) => recordResizeError(error.message));
  page.on("console", (message) => recordResizeError(message.text()));
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-dual", "agent");
  await settleChatProjections(page);
  await collapseArtifactPane(page);
  const panes = page.locator(".agent-chat-pane");
  await expect(panes).toHaveCount(1);
  for (const pane of await panes.all()) {
    const owner = await pane
      .locator(".acp-chat-projection")
      .getAttribute("data-chat-id");
    expect(owner).not.toBeNull();
    await edit(page, "completed", owner!);
  }
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/chatContainmentFixture.ts");
    fixture.installChatContainmentFixture(
      { name: "Codex", effort: true, context: true, goal: true, steer: true },
      true,
    );
  });
  for (const pane of await panes.all()) {
    await expect(pane.locator(".composer__queued")).toBeVisible();
    await expect(
      pane.getByRole("button", { name: /files? changed/ }),
    ).toBeVisible();
  }
  for (let cycle = 0; cycle < 3; cycle += 1) {
    await page.setViewportSize(
      [
        { width: 1888, height: 1096 },
        { width: 720, height: 800 },
        { width: 480, height: 560 },
      ][cycle]!,
    );
    for (const pane of await panes.all()) {
      const queue = pane.locator(".composer__queued");
      const composer = pane.locator("form.composer");
      const summary = pane.getByRole("region", {
        name: "Files edited in this chat",
      });
      const pill = summary.getByRole("button", { name: /files? changed/ });
      await pill.click();
      const input = composer.getByRole("textbox", { name: "Message agent" });
      await input.fill("A retained draft. ".repeat(cycle % 2 ? 80 : 1));
      const tray = await box(queue);
      expect(tray.height).toBeGreaterThan(0);
      expect(tray.height).toBeLessThanOrEqual(320);
      for (const zone of [
        await box(pill),
        await box(pane.locator(".chat-edit-summary-zone")),
      ])
        expect(zone.y + zone.height).toBeLessThanOrEqual(tray.y);
      expect(tray.y + tray.height).toBeLessThanOrEqual((await box(input)).y);
      await queue
        .getByRole("button", { name: /^Delete queued message:/ })
        .last()
        .click({ trial: true });
      const toolbar = await box(composer.locator(".composer__toolbar"));
      const dock = await box(pane.locator(".chat-dock"));
      expect(toolbar.y + toolbar.height).toBeLessThanOrEqual(
        dock.y + dock.height,
      );
      await input.click({ trial: true });
    }
    await page.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    );
  }
  resizeErrors.push(
    ...(await page.evaluate(
      () => (window as unknown as { __resizeLoops: string[] }).__resizeLoops,
    )),
  );
  expect(resizeErrors).toEqual([]);
});
