import { expect, test } from "@playwright/test";
import {
  dragAnswerPhrase,
  openSelectionScenario,
} from "./shared/transcriptSelection";

test.use({ deviceScaleFactor: 1 });
test("CP-42: temporary seeded right split, close confirmation, persisted opt-out", async ({
  page,
}) => {
  const workbench = await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  const right = page.locator(".agent-chat-pane").nth(1);
  const sideTab = page.locator(".agent-chat-center__side-tab");
  await expect(right).toContainText(
    "Side chats are temporary and disappear when you close the app.",
  );
  // The parent keeps its own header, title and rail selection; the side
  // pane's tab shares the header row; no project strip above its composer.
  const header = page.locator(".agent-chat-center__header");
  await expect(header).toHaveClass(/agent-chat-center__header--split/);
  const headerBox = (await header.boundingBox())!;
  const tabBox = (await sideTab.boundingBox())!;
  const rightBox = (await right.boundingBox())!;
  expect(tabBox.y).toBeGreaterThanOrEqual(headerBox.y);
  expect(tabBox.y + tabBox.height).toBeLessThanOrEqual(
    headerBox.y + headerBox.height + 1,
  );
  expect(Math.abs(tabBox.x - rightBox.x)).toBeLessThanOrEqual(12);
  await expect(header.locator(".agent-chat-center__side-tab")).toHaveCount(1);
  const current = page
    .getByRole("button", { name: /^Open chat / })
    .and(page.locator("[aria-current='page']"));
  await expect(current).toHaveCount(1);
  await expect(current).not.toHaveAccessibleName(/Side chat/);
  await expect(right.locator(".chat-home__scope")).toHaveCount(0);
  const homeIcon = (await right.locator(".side-chat-home svg").boundingBox())!;
  // Codex s2/f0244 draws the empty-state icon at about 23 px; Sage uses 24.
  expect(homeIcon.width).toBe(24);
  await expect(
    right.getByRole("button", { name: "1 selection", exact: true }),
  ).toBeVisible();
  await expect(right.locator("textarea")).toBeFocused();
  await expect(
    page.getByRole("separator", { name: "Resize chat panes" }),
  ).toHaveAttribute("aria-valuenow", "43");
  await expect(workbench).toHaveScreenshot("side-chat-empty.png");
  const prompt = "Explain the selection in this temporary discussion.";
  await right.locator("textarea").fill(prompt);
  await right
    .getByRole("button", { name: "Send message", exact: true })
    .click();
  await expect(sideTab.locator(".agent-chat-pane__title")).toHaveText(prompt);
  await expect(sideTab.locator(".spinner")).toBeVisible();
  // AP-C4: the running chat shows a spinner in its project group and Recents.
  await expect(page.locator(".project-chat__working").first()).toBeVisible();
  await expect(workbench).toHaveScreenshot("side-chat-running.png");
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/transcriptSelectionFixtures.ts");
    fixture.completeSideChatFixture();
  });
  await expect(right).toContainText(
    "This is a temporary discussion of the selected phrase.",
  );
  // A side chat can't fork, so its "…" offers only Copy (s2/f0244).
  await expect(
    right.getByRole("button", { name: "More actions", exact: true }),
  ).toBeVisible();
  await expect(sideTab.locator(".spinner")).toBeHidden();
  await expect(workbench).toHaveScreenshot("side-chat-completed.png");
  await sideTab
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Close side chat?" });
  await expect(
    dialog.getByRole("button", { name: "Close side chat", exact: true }),
  ).toBeFocused();
  const dialogBox = (await dialog.boundingBox())!;
  expect(dialogBox.height).toBeGreaterThanOrEqual(164);
  expect(dialogBox.height).toBeLessThanOrEqual(176);
  const checkbox = (await dialog
    .getByRole("checkbox", { name: "Don't ask again" })
    .boundingBox())!;
  expect(checkbox.width).toBe(14);
  // Codex s2/f0352: an unfilled outline box on the #2a2a2a dialog.
  const box = dialog.getByRole("checkbox", { name: "Don't ask again" });
  await expect(box).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  await expect(box).toHaveCSS("border-top-color", "rgb(133, 133, 133)");
  await expect(dialog).toHaveCSS("background-color", "rgb(42, 42, 42)");
  await expect(dialog.getByRole("button", { name: "Cancel" })).toHaveCSS(
    "background-color",
    "rgb(53, 53, 53)",
  );
  await expect(workbench).toHaveScreenshot("side-chat-close-confirm.png");
  await page.emulateMedia({ forcedColors: "active" });
  const systemText = await dialog.evaluate(() => {
    const probe = document.createElement("span");
    probe.style.color = "CanvasText";
    document.body.append(probe);
    const color = getComputedStyle(probe).color;
    probe.remove();
    return color;
  });
  await expect(box).toHaveCSS("border-top-color", systemText);
  await box.check();
  await expect(box).toHaveCSS("background-color", systemText);
  await box.uncheck();
  await page.emulateMedia({ forcedColors: "none" });
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await sideTab
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await dialog.getByRole("checkbox", { name: "Don't ask again" }).check();
  await dialog
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  await page
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
  await expect(dialog).toBeHidden();
});

test("CP-42: at 480 the side tab's ✕ stays clear of the right-pane toggle", async ({
  page,
}) => {
  await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  await page.setViewportSize({ width: 480, height: 1000 });
  const close = page
    .locator(".agent-chat-center__side-tab")
    .getByRole("button", { name: "Close side chat", exact: true });
  // Wait for the narrow single-pane layout before measuring.
  await expect
    .poll(async () => {
      const box = await close.boundingBox();
      return box ? box.x + box.width : Infinity;
    })
    .toBeLessThanOrEqual(480);
  const closeBox = (await close.boundingBox())!;
  const toggle = (await page
    .locator(".agent-artifact-header-slot button")
    .boundingBox())!;
  expect(closeBox.x + closeBox.width).toBeLessThanOrEqual(toggle.x - 4);
  // The ✕ takes the pointer itself, never the toggle above it.
  await close.click({ trial: true });
  const tab = (await page.locator(".side-chat-tab").boundingBox())!;
  expect(tab.width).toBeLessThanOrEqual(213);
});

test("CP-42: a running temporary chat closes only after confirmation", async ({
  page,
}) => {
  await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  const right = page.locator(".agent-chat-pane").nth(1);
  const sideTab = page.locator(".agent-chat-center__side-tab");
  await right.locator("textarea").fill("A running temporary question");
  await right
    .getByRole("button", { name: "Send message", exact: true })
    .click();
  await expect(sideTab.locator(".spinner")).toBeVisible();
  await sideTab
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Close side chat?" });
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  await dialog
    .getByRole("button", { name: "Close side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
});
