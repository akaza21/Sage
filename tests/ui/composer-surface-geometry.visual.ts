import { expect, test } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

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
    const scope = page.getByRole("group", {
      name: "Project scope",
      exact: true,
    });
    await expect(scope).toBeVisible();
    const composer = page.locator("form.composer");
    const geometry = await composer.evaluate((form) => {
      const scope = document.querySelector(
        '[role="group"][aria-label="Project scope"]',
      )!;
      const body = form.querySelector(".composer__body")!;
      const style = getComputedStyle(form);
      const box = form.getBoundingClientRect();
      const row = scope.getBoundingClientRect();
      const input = body.getBoundingClientRect();
      const corners = (element: Element) => {
        const css = getComputedStyle(element);
        return [
          css.borderTopLeftRadius,
          css.borderTopRightRadius,
          css.borderBottomRightRadius,
          css.borderBottomLeftRadius,
        ];
      };
      return {
        token: style.getPropertyValue("--radius-xl").trim(),
        corners: corners(form),
        ownsScope: form.contains(scope),
        touching: [
          getComputedStyle(scope).borderBottomLeftRadius,
          getComputedStyle(scope).borderBottomRightRadius,
          ...corners(body).slice(0, 2),
        ],
        edges: [
          row.left - box.left,
          box.right - row.right,
          input.left - box.left,
          box.right - input.right,
        ],
        border: Number.parseFloat(style.borderLeftWidth),
        borderToken: Number.parseFloat(
          style.getPropertyValue("--border-width"),
        ),
        childBackgrounds: [
          getComputedStyle(scope).backgroundColor,
          getComputedStyle(body).backgroundColor,
        ],
        childShadows: [
          getComputedStyle(scope).boxShadow,
          getComputedStyle(body).boxShadow,
        ],
        join: input.top - row.bottom,
        childBorders: [
          getComputedStyle(scope).borderBottomWidth,
          getComputedStyle(body).borderTopWidth,
        ],
      };
    });
    expect.soft(geometry.corners).toEqual(Array(4).fill(geometry.token));
    expect.soft(geometry.ownsScope).toBe(true);
    expect.soft(geometry.touching).toEqual(Array(4).fill("0px"));
    expect.soft(geometry.childBorders).toEqual(["0px", "0px"]);
    expect.soft(geometry.border).toBe(geometry.borderToken);
    expect
      .soft(geometry.childBackgrounds)
      .toEqual(Array(2).fill("rgba(0, 0, 0, 0)"));
    expect.soft(geometry.childShadows).toEqual(["none", "none"]);
    expect.soft(geometry.join).toBe(0);
    for (const edge of geometry.edges) expect.soft(edge).toBe(geometry.border);

    if (!(await page.locator(".agent-artifact-pane").isVisible()))
      await page
        .getByRole("button", { name: /Show files and preview/ })
        .click();
    for (const selector of [".agent-chat-center", ".agent-artifact-pane"]) {
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
    }
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
    expect(rules).toEqual([]);

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
