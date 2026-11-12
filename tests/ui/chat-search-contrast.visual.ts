import { expect, test, type Locator, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// The Search chats dialog is translucent glass over a scrim, so what shows
// through decides its contrast. Every text role must stay WCAG AA (4.5:1)
// over the worst underlays: white and black.

type Underlay = "white" | "black";

/** Lowest contrast ratio of any visible text in the dialog. */
function lowestTextContrast(dialog: Locator, underlay: Underlay) {
  return dialog.evaluate((root, underlay) => {
    type Rgba = [number, number, number, number];
    const parse = (value: string): Rgba => {
      const [r, g, b, a = 1] = value.match(/[\d.]+/g)!.map(Number);
      return [r, g, b, a];
    };
    const over = (top: Rgba, bottom: Rgba): Rgba => {
      const alpha = top[3] + bottom[3] * (1 - top[3]);
      const mix = (index: number) =>
        (top[index] * top[3] + bottom[index] * bottom[3] * (1 - top[3])) /
        alpha;
      return [mix(0), mix(1), mix(2), alpha];
    };
    const luminance = ([r, g, b]: Rgba) => {
      const linear = (channel: number) => {
        const scaled = channel / 255;
        return scaled <= 0.04045
          ? scaled / 12.92
          : ((scaled + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    };
    const ratio = (a: Rgba, b: Rgba) => {
      const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
      return (high + 0.05) / (low + 0.05);
    };

    const base: Rgba = underlay === "white" ? [255, 255, 255, 1] : [0, 0, 0, 1];
    const backdrop = parse(
      getComputedStyle(root, "::backdrop").backgroundColor,
    );
    const behindDialog = over(backdrop, base);
    const backgroundOf = (element: Element): Rgba => {
      const chain: Element[] = [];
      for (
        let node: Element | null = element;
        node;
        node = node.parentElement
      ) {
        chain.unshift(node);
        if (node === root) break;
      }
      return chain.reduce(
        (below, node) =>
          over(parse(getComputedStyle(node).backgroundColor), below),
        behindDialog,
      );
    };

    let lowest = { ratio: Infinity, text: "" };
    for (const element of [root, ...root.querySelectorAll("*")]) {
      const text =
        element instanceof HTMLInputElement
          ? element.value
          : [...element.childNodes]
              .filter((node) => node.nodeType === Node.TEXT_NODE)
              .map((node) => node.textContent!.trim())
              .join(" ")
              .trim();
      if (!text || element.getClientRects().length === 0) continue;
      const background = backgroundOf(element);
      const foreground = over(
        parse(getComputedStyle(element).color),
        background,
      );
      const contrast = ratio(foreground, background);
      if (contrast < lowest.ratio) lowest = { ratio: contrast, text };
    }
    return lowest;
  }, underlay);
}

async function openSearchChats(page: Page) {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-dual", "agent");
  await page
    .locator("header.title-bar")
    .getByRole("button", { name: "Search chats" })
    .click();
  const dialog = page.getByRole("dialog", { name: "Search chats" });
  await expect(dialog).toBeVisible();
  return dialog;
}

test.describe("Search chats contrast", () => {
  test.use({ reducedMotion: "reduce" });

  for (const underlay of ["white", "black"] as const) {
    test(`keeps every text role AA readable over a ${underlay} underlay`, async ({
      page,
    }) => {
      const dialog = await openSearchChats(page);
      // A search with no native backend reports its error inline, so the
      // error role is on screen with the field, picker and actions.
      await dialog.getByRole("searchbox").fill("chat");
      await expect(dialog.locator(".composer__error")).toBeVisible();

      const atRest = await lowestTextContrast(dialog, underlay);
      expect(atRest.ratio, atRest.text).toBeGreaterThanOrEqual(4.5);

      for (const button of await dialog.getByRole("button").all()) {
        await button.hover();
        const hovered = await lowestTextContrast(dialog, underlay);
        expect(hovered.ratio, hovered.text).toBeGreaterThanOrEqual(4.5);
      }
    });
  }
});
