import { expect, test, type Page } from "@playwright/test";

import type {
  VisualScenarioMode,
  VisualScenarioName,
} from "./fixtures/visualScenarioTypes";
import { openScenario } from "./shared/visualBrowserPrimitives";

// Motion contracts read from the rendered app: every element and
// pseudo-element's computed animation and transition, plus the keyframes of
// the animations actually running.

const SCENARIOS: ReadonlyArray<[VisualScenarioName, VisualScenarioMode]> = [
  // Live activity: rail and chip spinners, shimmer, plan pulses, follow pill.
  ["agent-activity-family", "agent"],
  ["agent-thinking-live", "agent"],
  // Settled turn: Worked disclosure, turn actions, changed-file card.
  ["agent-preview-starting", "agent"],
  ["editor", "editor"],
];

async function openWithSearchDialog(
  page: Page,
  name: VisualScenarioName,
  mode: VisualScenarioMode,
) {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, name, mode);
  if (mode !== "agent") return;
  await page
    .locator("header.title-bar")
    .getByRole("button", { name: "Search chats" })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Search chats" }),
  ).toBeVisible();
}

/** Elements and pseudo-elements that would move: label plus timing. */
function movingElements(page: Page) {
  return page.evaluate(() => {
    const seconds = (list: string) =>
      list.split(",").some((value) => Number.parseFloat(value) > 0);
    const moving: string[] = [];
    for (const element of document.querySelectorAll("*")) {
      for (const pseudo of [null, "::before", "::after", "::backdrop"]) {
        const style = getComputedStyle(element, pseudo);
        const animated =
          style.animationName !== "none" &&
          (seconds(style.animationDuration) || seconds(style.animationDelay));
        if (
          animated ||
          seconds(style.transitionDuration) ||
          seconds(style.transitionDelay)
        ) {
          moving.push(
            `${element.tagName.toLowerCase()}.${[...element.classList].join(".")}${pseudo ?? ""}`,
          );
        }
      }
    }
    return {
      moving,
      running: document
        .getAnimations()
        .filter((animation) => animation.playState === "running").length,
    };
  });
}

test.describe("Motion invariants", () => {
  for (const [name, mode] of SCENARIOS) {
    test(`reduced motion stops every animation and transition (${name})`, async ({
      page,
    }) => {
      await page.emulateMedia({ reducedMotion: "reduce" });
      await openWithSearchDialog(page, name, mode);
      expect(await movingElements(page)).toEqual({ moving: [], running: 0 });
    });
  }

  test("the same surfaces do move with motion allowed", async ({ page }) => {
    // Guards the sweep above: it must see motion when motion is on.
    await openWithSearchDialog(page, "agent-activity-family", "agent");
    const { moving, running } = await movingElements(page);
    expect(moving.length).toBeGreaterThan(0);
    expect(running).toBeGreaterThan(0);
  });

  for (const [name, mode] of SCENARIOS.filter(([, mode]) => mode === "agent")) {
    test(`transcript and dialog motion never animates layout (${name})`, async ({
      page,
    }) => {
      await openWithSearchDialog(page, name, mode);
      const layoutMotion = await page.evaluate(() => {
        // Animating these reflows the transcript on every frame.
        const layout =
          /^(?:width|height|min-|max-|margin|padding|inset|top|right|bottom|left|grid|flex|gap|border(?:-\w+)?-width|font-size|line-height|all$)/;
        const scope = [
          ...document.querySelectorAll(".message-list, dialog[open]"),
        ];
        const inScope = (node: Node | null) =>
          node !== null && scope.some((root) => root.contains(node));
        const found: string[] = [];
        for (const root of scope) {
          for (const element of [root, ...root.querySelectorAll("*")]) {
            for (const pseudo of [null, "::before", "::after"]) {
              const style = getComputedStyle(element, pseudo);
              const durations = style.transitionDuration.split(", ");
              style.transitionProperty
                .split(", ")
                .forEach((property, index) => {
                  const duration = durations[index % durations.length];
                  if (Number.parseFloat(duration) > 0 && layout.test(property))
                    found.push(
                      `${element.className}${pseudo ?? ""} ${property}`,
                    );
                });
            }
          }
        }
        for (const animation of document.getAnimations()) {
          const effect = animation.effect as KeyframeEffect | null;
          if (!effect || !inScope(effect.target)) continue;
          for (const keyframe of effect.getKeyframes()) {
            for (const property of Object.keys(keyframe)) {
              const css = property.replace(
                /[A-Z]/g,
                (c) => `-${c.toLowerCase()}`,
              );
              if (layout.test(css)) found.push(`keyframes ${css}`);
            }
          }
        }
        return {
          checked: document
            .getAnimations()
            .filter((a) =>
              inScope((a.effect as KeyframeEffect | null)?.target ?? null),
            ).length,
          found,
        };
      });
      expect(layoutMotion.found).toEqual([]);
      // The search dialog reveal at least is running, so the sweep is live.
      expect(layoutMotion.checked).toBeGreaterThan(0);
    });
  }
});
