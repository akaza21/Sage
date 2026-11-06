import { expect, test } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

for (const width of [480, 1280]) {
  test(`expanded edits leave a bounded dock and reachable controls at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 560 });
    await openScenario(page, "diff", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const fixture = await import("/tests/ui/fixtures/cp30cEditFixture.ts");
      fixture.installCp30cDisclosure("partial");
    });
    const summary = page.getByRole("region", {
      name: "Files edited in this chat",
    });
    await summary.getByRole("button", { name: /files changed/ }).click();
    const dock = page.locator(".chat-dock");
    const bounds = (await dock.boundingBox())!;
    const surface = (await page.locator(".chat-surface").boundingBox())!;
    expect(bounds.height).toBeLessThanOrEqual(surface.height * 0.6 + 1);
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(
      surface.y + surface.height,
    );
    const input = page.getByRole("textbox", { name: "Message agent" });
    await input.fill("Review these edits");
    for (const control of [
      input,
      dock.getByRole("button", { name: "Access options", exact: true }),
      dock.getByRole("button", { name: "Harness", exact: true }),
      dock.getByRole("button", { name: "Send message", exact: true }),
    ]) {
      const box = (await control.boundingBox())!;
      expect(box.y).toBeGreaterThanOrEqual(bounds.y);
      expect(box.y + box.height).toBeLessThanOrEqual(bounds.y + bounds.height);
      expect(box.x).toBeGreaterThanOrEqual(bounds.x);
      expect(box.x + box.width).toBeLessThanOrEqual(bounds.x + bounds.width);
      await control.click({ trial: true });
    }
    await summary
      .getByRole("button", {
        name: "Open diff Sage / src/disclosure-15.tsx",
        exact: true,
      })
      .click({ trial: true });
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      useWorkbenchStore.setState((state) => ({
        chatUiById: {
          ...state.chatUiById,
          [state.activeTabId!]: {
            ...state.chatUiById[state.activeTabId!],
            queuedPrompts: Array.from({ length: 8 }, (_, index) => ({
              id: `queue-${index}`,
              text: `Follow-up ${index}`,
              attachedContext: [],
              bindingId: "fixture",
              modelId: null,
              status: "blocked" as const,
            })),
          },
        },
      }));
    });
    await page
      .getByLabel("Queued messages", { exact: true })
      .getByRole("button", {
        name: "More options for queued message: Follow-up 0",
        exact: true,
      })
      .click({ trial: true });
    await input.click({ trial: true });
    const queuedBounds = (await dock.boundingBox())!;
    expect(queuedBounds.height).toBeLessThanOrEqual(surface.height * 0.6 + 1);
  });
}

for (const viewport of [
  { width: 900, height: 800 },
  { width: 1280, height: 800 },
  { width: 1872, height: 1080 },
  { width: 2560, height: 1440 },
  { width: 1920, height: 1080 },
]) {
  test(`attached composer and inset cards at ${viewport.width}x${viewport.height}`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const { acpProjectionStoreFor } =
        await import("/src/app/acpProjectionRegistry.ts");
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const store = acpProjectionStoreFor(
        useWorkbenchStore.getState().activeTabId,
      );
      const { page: current, scope } = store.getState();
      if (!current || !scope) throw new Error("Expected a loaded chat");
      store.getState().applyAuthoritativePage(scope, {
        ...current,
        turns: [],
        nextCursor: null,
      });
    });
    const composer = page.locator("form.composer");
    await expect(composer).toBeVisible();
    const geometry = await composer.evaluate((form) => {
      const dock = form.closest(".chat-dock")!;
      const body = form.querySelector(".composer__body")!;
      const style = getComputedStyle(dock);
      const box = dock.getBoundingClientRect();
      const input = body.getBoundingClientRect();
      const css = getComputedStyle(body);
      return {
        token: style.getPropertyValue("--radius-xl").trim(),
        corners: [
          style.borderTopLeftRadius,
          style.borderTopRightRadius,
          style.borderBottomRightRadius,
          style.borderBottomLeftRadius,
        ],
        edges: [input.left - box.left, box.right - input.right],
        border: Number.parseFloat(style.borderLeftWidth),
        borderToken: Number.parseFloat(
          style.getPropertyValue("--border-width"),
        ),
        bodyPaint: [css.backgroundColor, css.boxShadow, css.borderTopWidth],
      };
    });
    // The new chat's input is painted by the dock alone: no inner card,
    // shadow or border, inset by exactly the dock's border.
    expect.soft(geometry.corners).toEqual(Array(4).fill(geometry.token));
    expect.soft(geometry.border).toBe(geometry.borderToken);
    expect
      .soft(geometry.bodyPaint)
      .toEqual(["rgba(0, 0, 0, 0)", "none", "0px"]);
    for (const edge of geometry.edges) expect.soft(edge).toBe(geometry.border);

    const assertInsetCard = async (selector: string) => {
      const card = page.locator(selector);
      await expect(card).toBeVisible();
      const bounds = await card.evaluate((element) => {
        const css = getComputedStyle(element);
        const box = element.getBoundingClientRect();
        const host = document
          .querySelector(".workbench__body")!
          .getBoundingClientRect();
        const track = document
          .querySelector(".center-region")!
          .getBoundingClientRect();
        return {
          radius: [
            css.borderTopLeftRadius,
            css.borderTopRightRadius,
            css.borderBottomLeftRadius,
            css.borderBottomRightRadius,
          ],
          token: css.getPropertyValue("--radius-xl").trim(),
          inset: Number.parseFloat(css.getPropertyValue("--space-2")),
          top: box.top - host.top,
          bottom: host.bottom - box.bottom,
          right: host.right - box.right,
          chatLeft: box.left - track.left,
          chatRight: track.right - box.right,
        };
      });
      expect.soft(bounds.radius).toEqual(Array(4).fill(bounds.token));
      expect.soft(bounds.top).toBe(bounds.inset);
      expect.soft(bounds.bottom).toBe(bounds.inset);
      if (selector === ".agent-chat-center") {
        expect.soft(bounds.chatLeft).toBe(bounds.inset);
        expect.soft(bounds.chatRight).toBe(bounds.inset);
      }
      if (selector === ".agent-artifact-pane")
        expect.soft(bounds.right).toBe(bounds.inset);
    };
    await assertInsetCard(".agent-chat-center");
    const rules = await composer.evaluate((form) => {
      const bottom = form.getBoundingClientRect().bottom;
      return [...document.querySelectorAll("body *")]
        .filter((element) => {
          const box = element.getBoundingClientRect();
          if (
            box.width < innerWidth * 0.9 ||
            box.top < bottom ||
            box.height === 0
          )
            return false;
          const css = getComputedStyle(element);
          return (
            element.matches('hr, [role="separator"]') ||
            [css.borderTopWidth, css.borderBottomWidth].some(
              (width) => Number.parseFloat(width) > 0,
            )
          );
        })
        .map((element) => element.className || element.tagName);
    });
    // The window status bar owns the single full-width divider below the
    // inset cards; the composer must not introduce another separator.
    expect(rules).toEqual(["status-bar"]);
    const status = page.locator(".status-bar");
    await expect(status).toHaveCSS(
      "border-top-width",
      await status.evaluate((element) =>
        getComputedStyle(element).getPropertyValue("--border-width").trim(),
      ),
    );
    await expect(status).toHaveCSS("border-top-style", "solid");

    if (!(await page.locator(".agent-artifact-pane").isVisible()))
      await page
        .getByRole("button", { name: /Show files and preview/ })
        .click();
    await assertInsetCard(".agent-artifact-pane");
    const center = page.locator(".agent-chat-center");
    if (await page.locator(".workbench__body--artifact-overlay").count()) {
      await expect(center).toBeHidden();
      await page
        .getByRole("button", {
          name: "Collapse files, changes, and preview",
          exact: true,
        })
        .click();
      await expect(center).toBeVisible();
      await assertInsetCard(".agent-chat-center");
      await page
        .getByRole("button", { name: /Show files and preview/ })
        .click();
      await expect(center).toBeHidden();
    } else await assertInsetCard(".agent-chat-center");

    await page
      .getByRole("button", { name: "Expand resource pane", exact: true })
      .click();
    const expanded = await page
      .locator(".agent-artifact-pane")
      .evaluate((element) => {
        const box = element.getBoundingClientRect();
        const host = document
          .querySelector(".workbench__body")!
          .getBoundingClientRect();
        const css = getComputedStyle(element);
        return {
          edges: [
            box.left - host.left,
            host.right - box.right,
            box.top - host.top,
            host.bottom - box.bottom,
          ],
          inset: Number.parseFloat(css.getPropertyValue("--space-2")),
          radius: [
            css.borderTopLeftRadius,
            css.borderTopRightRadius,
            css.borderBottomLeftRadius,
            css.borderBottomRightRadius,
          ],
          token: css.getPropertyValue("--radius-xl").trim(),
        };
      });
    expect(expanded.edges).toEqual(Array(4).fill(expanded.inset));
    expect(expanded.radius).toEqual(Array(4).fill(expanded.token));
  });
}
