import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

type Resource = "closed" | "file" | "diff" | "preview";
const geometries = [
  {
    width: 1888,
    height: 1096,
    navigation: 385,
    closedChat: 1495,
    openChat: 637,
  },
  {
    width: 1280,
    height: 1000,
    navigation: 385,
    closedChat: 887,
    openChat: 360,
  },
  { width: 720, height: 1000, navigation: 332, closedChat: 388, openChat: 388 },
  { width: 480, height: 1000, navigation: 54, closedChat: 426, openChat: 426 },
];

async function box(locator: Locator) {
  await expect(locator).toBeVisible();
  const bounds = await locator.boundingBox();
  expect(bounds).not.toBeNull();
  return bounds!;
}

async function expectHitTarget(control: Locator, minimum = 24) {
  const bounds = await box(control);
  expect(bounds.width).toBeGreaterThanOrEqual(minimum);
  expect(bounds.height).toBeGreaterThanOrEqual(minimum);
  expect(
    await control.evaluate((node) => {
      const rect = node.getBoundingClientRect();
      return node.contains(
        document.elementFromPoint(
          rect.x + rect.width / 2,
          rect.y + rect.height / 2,
        ),
      );
    }),
  ).toBe(true);
}

async function expectComposerControls(page: Page) {
  for (const composer of await page.locator("form.composer").all()) {
    const bounds = await box(composer);
    const controls = [
      composer.getByRole("button", {
        name: "Access options",
        exact: true,
      }),
      composer.getByRole("button", { name: "Harness", exact: true }),
      composer.getByRole("button", { name: "Model", exact: true }),
      composer.locator(".composer__mic"),
      composer.locator(".send-button"),
    ];
    const rectangles = [];
    for (const control of controls) {
      await expectHitTarget(control, bounds.width <= 520 ? 24 : 22);
      const rect = await box(control);
      expect(rect.x).toBeGreaterThanOrEqual(bounds.x);
      expect(rect.x + rect.width).toBeLessThanOrEqual(bounds.x + bounds.width);
      expect(rect.y).toBeGreaterThanOrEqual(bounds.y);
      expect(rect.y + rect.height).toBeLessThanOrEqual(
        bounds.y + bounds.height,
      );
      for (const sibling of rectangles) {
        expect(
          Math.min(rect.x + rect.width, sibling.x + sibling.width) -
            Math.max(rect.x, sibling.x) <=
            0.5 ||
            Math.min(rect.y + rect.height, sibling.y + sibling.height) -
              Math.max(rect.y, sibling.y) <=
              0.5,
        ).toBe(true);
      }
      rectangles.push(rect);
    }
  }
}

async function openResource(
  page: Page,
  kind: Exclude<Resource, "closed">,
  ownerId: string,
) {
  await page.evaluate(async (resourceKind) => {
    const path = "/tests/ui/fixtures/cp27bResourceFixture.ts";
    const fixture = await import(path);
    await fixture.installCp27bResource(resourceKind);
  }, kind);
  const pane = page.getByLabel("Agent artifacts", { exact: true });
  const tab =
    kind === "preview"
      ? pane.getByRole("tab", { name: "Preview", exact: true })
      : pane.locator(`.tab[data-resource-kind="${kind}"]`).getByRole("tab");
  await tab.click();
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await expect(pane).toHaveAttribute("data-owner-chat-id", ownerId);
  if (kind === "file")
    await expect(pane.locator(".file-surface .cm-editor")).toBeVisible();
  if (kind === "diff")
    await expect(
      pane.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
  if (kind === "preview")
    await expect(pane.getByTitle("Sage browser")).toBeVisible();
  return pane;
}

for (const geometry of geometries) {
  for (const two of [false, true]) {
    for (const resource of ["closed", "file", "diff", "preview"] as const) {
      test(`Agent geometry ${geometry.width}px, ${two ? "two selected" : "single"}, ${resource}`, async ({
        page,
      }) => {
        const {
          width,
          height,
          navigation: navigationWidth,
          closedChat,
          openChat,
        } = geometry;
        await page.setViewportSize({ width, height });
        await openScenario(page, two ? "agent-dual" : "agent-single", "agent");
        await settleChatProjections(page);
        // agent-dual's secondary chat has an active turn, so its composer is
        // disabled by the fixture; the draft and resource owner use the
        // primary chat, which stays focused while both chats are selected.
        if (two)
          await page.evaluate(async () => {
            const { useWorkbenchStore } = await import("/src/app/store.ts");
            await useWorkbenchStore
              .getState()
              .activateTab("visual-chat-primary");
          });
        const ownerId = two
          ? "visual-chat-primary"
          : "visual-chat-agent-single";
        const navigation = page.locator(".project-sidebar");
        const center = page.locator(".center-region");
        const trigger = page.getByRole("button", {
          name: /^Show files and preview/,
        });
        await expectHitTarget(trigger);
        expect(
          await trigger.evaluate(
            (node) => node.closest(".agent-chat-center__header") !== null,
          ),
        ).toBe(true);
        const draft = page.locator(
          ".agent-chat-pane--focused form.composer textarea",
        );
        await draft.fill("CP-27b draft survives resources and resizing");
        if (resource !== "closed") await openResource(page, resource, ownerId);
        const nav = await box(navigation);
        const chat = await box(center);
        expect(nav.x).toBe(0);
        expect(nav.width).toBe(navigationWidth);
        expect(chat.x).toBe(nav.width + (width > 760 ? 8 : 0));
        expect(chat.x + chat.width).toBeLessThanOrEqual(width);
        const inline = resource !== "closed" && width >= 1280;
        expect(chat.width).toBe(resource === "closed" ? closedChat : openChat);
        await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
        for (const pane of await page.locator(".agent-chat-pane").all()) {
          const lane = await box(pane);
          const items = pane.locator(".acp-turn__items");
          expect(await items.count()).toBeGreaterThan(0);
          for (const item of await items.all()) {
            const reading = await item.boundingBox();
            expect(reading).not.toBeNull();
            expect(reading!.width).toBeLessThanOrEqual(660);
            expect(reading!.x).toBeGreaterThanOrEqual(lane.x);
            expect(reading!.x + reading!.width).toBeLessThanOrEqual(
              lane.x + lane.width,
            );
          }
          if (width === 1888 && resource === "closed")
            expect((await items.first().boundingBox())!.width).toBe(660);
        }
        for (const composer of await page.locator("form.composer").all()) {
          const bounds = await box(composer);
          expect(bounds.width).toBeLessThanOrEqual(660);
          if (width === 1888 && resource === "closed")
            expect(bounds.width).toBe(660);
          expect(bounds.x).toBeGreaterThanOrEqual(chat.x);
          expect(bounds.x + bounds.width).toBeLessThanOrEqual(
            chat.x + chat.width,
          );
          if (width === 720) expect(bounds.width).toBe(356);
          if (width === 480) expect(bounds.width).toBe(394);
        }
        if (inline) await expectComposerControls(page);
        if (resource !== "closed") {
          const pane = page.getByLabel("Agent artifacts", { exact: true });
          const bounds = await box(pane);
          expect(bounds.x + bounds.width).toBe(width);
          if (inline) expect(bounds.x).toBe(chat.x + chat.width + 8);
          else {
            expect(bounds.width).toBe(width === 480 ? 420 : chat.width);
            if (chat.width >= 320) {
              expect(bounds.x).toBeGreaterThanOrEqual(nav.x + nav.width);
              await expect(navigation).not.toHaveAttribute("inert");
              await expect(pane).not.toHaveAttribute("aria-modal");
            } else {
              await expect(navigation).toHaveAttribute("inert", "");
              await expect(pane).toHaveAttribute("aria-modal", "true");
            }
            expect(bounds.y).toBe(nav.y);
            expect(bounds.y + bounds.height).toBe(nav.y + nav.height);
            await expect(center).toHaveAttribute("inert", "");
          }
          const close = pane.getByRole("button", {
            name: "Collapse files, changes, and preview",
          });
          await expectHitTarget(close);
          await close.click();
          await expect(
            page.locator(".agent-artifact-pane__canvas"),
          ).toBeHidden();
          await expect(trigger).toBeFocused();
          await expect(center).not.toHaveAttribute("inert");
        } else
          await expect(page.locator(".agent-artifact-pane")).toHaveCount(0);
        await expect(draft).toHaveValue(
          "CP-27b draft survives resources and resizing",
        );
        await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
        await expectHitTarget(
          page.getByRole("button", { name: "New chat", exact: true }),
        );
        await expectHitTarget(trigger);
        await expectComposerControls(page);
      });
    }
  }
}

for (const width of [720, 480]) {
  test(`Preview iframe Tab stays inside resources at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1000 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    const pane = await openResource(
      page,
      "preview",
      "visual-chat-agent-single",
    );
    const frame = page.frameLocator('iframe[title="Sage browser"]');
    await frame.locator("body").evaluate((body) => {
      const button = body.ownerDocument.createElement("button");
      button.textContent = "Iframe keyboard target";
      body.append(button);
    });
    const target = frame.getByRole("button", {
      name: "Iframe keyboard target",
    });
    await target.focus();
    await page.keyboard.press("Tab");
    await expect(pane).toBeFocused();
    await target.focus();
    await page.keyboard.press("Shift+Tab");
    await expect
      .poll(() =>
        pane.evaluate((node) => node.contains(document.activeElement)),
      )
      .toBe(true);
    // At overlay widths Reload lives in the Preview controls menu; its
    // always-visible trigger is the in-pane control Escape is pressed from.
    const controls = pane.getByRole("button", { name: "Preview controls" });
    await controls.focus();
    await page.keyboard.press("Escape");
    await expect(
      page.getByRole("button", { name: /^Show files and preview/ }),
    ).toBeFocused();
    if (width === 480) {
      await page
        .getByRole("button", { name: /^Show files and preview/ })
        .click();
      await page.mouse.click(16, 200);
      // Collapsed navigation leaves room for a nonmodal resource overlay.
      await expect(pane).toBeVisible();
      await pane
        .getByRole("button", { name: "Collapse files, changes, and preview" })
        .click();
      await expect(pane).toBeHidden();
    }
  });
}

for (const forcedColors of ["none", "active"] as const) {
  test(`header resource menu stays in the viewport while navigation narrows (${forcedColors})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await openScenario(page, "agent-dual", "agent");
    await settleChatProjections(page);
    const trigger = page.getByRole("button", { name: "Show chat resources" });
    await trigger.click();
    const menu = page.getByRole("dialog", {
      name: "Chat resources",
      exact: true,
    });
    for (const { width, height, navigation } of geometries) {
      await page.setViewportSize({ width, height });
      // The popover repositions from its resize listeners after the resize
      // frame; wait for that frame instead of sampling the pre-resize box.
      await expect
        .poll(async () => {
          const settled = await box(menu);
          return settled.x + settled.width <= width - 8;
        })
        .toBe(true);
      const bounds = await box(menu);
      expect(bounds.x).toBeGreaterThanOrEqual(8);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width - 8);
      expect(bounds.y).toBeGreaterThanOrEqual(8);
      expect(bounds.y + bounds.height).toBeLessThanOrEqual(height - 8);
      expect((await box(page.locator(".project-sidebar"))).width).toBe(
        navigation,
      );
    }
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();
  });

  test(`multiple open chats retain the selected draft and owner without regrowing panes (${forcedColors})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await openScenario(page, "agent-dual", "agent");
    await settleChatProjections(page);
    const panes = page.locator(".agent-chat-pane");
    const firstInput = panes.nth(0).locator("form.composer textarea");
    await firstInput.fill("First draft");
    // The fixture's secondary chat has an active turn; its composer stays
    // disabled and must not gain a draft through resizing.
    await expect(panes).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    const ownerName = "Chat pane Agent navigation polish";
    const ownerId = "visual-chat-primary";
    await page
      .getByTitle("Open chat Agent navigation polish", { exact: true })
      .click();
    const pane = await openResource(page, "file", ownerId);
    for (const viewport of [
      { width: 1280, height: 1000 },
      { width: 720, height: 1000 },
      { width: 480, height: 560 },
    ]) {
      await page.setViewportSize(viewport);
      await expect(panes).toHaveCount(1);
      await expect(panes.first()).toHaveAttribute("aria-label", ownerName!);
      await expect(pane).toHaveAttribute("data-owner-chat-id", ownerId!);
      // The layout re-fits from the window resize listener after the resize
      // frame; wait for it instead of sampling the pre-resize box.
      await expect
        .poll(async () => {
          const settled = await box(pane);
          return settled.x + settled.width <= viewport.width;
        })
        .toBe(true);
      const rect = await box(pane);
      expect(rect.x).toBeGreaterThanOrEqual(0);
      expect(rect.x + rect.width).toBeLessThanOrEqual(viewport.width);
      expect(rect.y + rect.height).toBeLessThanOrEqual(viewport.height);
      if (viewport.width < 1280) await expect(pane).toBeFocused();
    }
    await page.keyboard.press("Escape");
    const trigger = page.getByRole("button", {
      name: /^Show files and preview/,
    });
    await expect(trigger).toBeFocused();
    await expect(panes.first().locator("form.composer textarea")).toHaveValue(
      "First draft",
    );
    await trigger.click();
    await expect(pane).toHaveAttribute("data-owner-chat-id", ownerId!);
    await page.keyboard.press("Tab");
    await expect(pane.getByRole("tab").first()).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect
      .poll(() =>
        pane.evaluate((node) => node.contains(document.activeElement)),
      )
      .toBe(true);
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();
    await page.setViewportSize({ width: 1888, height: 1096 });
    await expect(panes).toHaveCount(1);
    await expect(firstInput).toHaveValue("First draft");
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
  });
}
