import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";
import { revealWorkedContent } from "./agentVisualHelpers";
import type { ChatContainmentProfile } from "./fixtures/chatContainmentFixture";

const profiles: ChatContainmentProfile[] = [
  { name: "Codex", effort: true, context: true, goal: true, steer: true },
  { name: "Claude", effort: true, context: false, goal: false, steer: false },
  { name: "Pi", effort: true, context: true, goal: false, steer: true },
  {
    name: "OpenCode",
    effort: false,
    context: false,
    goal: false,
    steer: false,
  },
  { name: "Oh My Pi", effort: true, context: true, goal: false, steer: true },
];
const viewports = [
  { width: 480, height: 1000 },
  { width: 720, height: 1000 },
  { width: 1888, height: 1096 },
  { width: 480, height: 560 },
];

async function prepare(
  page: Page,
  profile: ChatContainmentProfile,
  two: boolean,
  live: boolean,
) {
  await openScenario(page, two ? "agent-dual" : "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(
    async ({ profile, live }) => {
      const fixturePath = "/tests/ui/fixtures/chatContainmentFixture.ts";
      const fixture = await import(fixturePath);
      fixture.installChatContainmentFixture(profile, live);
    },
    { profile, live },
  );
  await expect(
    page.locator("form.composer").first().getByRole("textbox"),
  ).toHaveValue("Draft survives resizing");
}

async function expectControls(
  owner: Locator,
  controls: Locator,
  viewport: { width: number; height: number },
  scrollControls = false,
) {
  const boxes = [];
  for (const control of await controls.all()) {
    if (!(await control.isVisible())) continue;
    if (scrollControls) await control.scrollIntoViewIfNeeded();
    const boundary = await owner.boundingBox();
    expect(boundary).not.toBeNull();
    const box = await control.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.width).toBeGreaterThanOrEqual(24);
    expect(box!.height).toBeGreaterThanOrEqual(24);
    expect(box!.x).toBeGreaterThanOrEqual(Math.max(0, boundary!.x));
    expect(box!.y).toBeGreaterThanOrEqual(Math.max(0, boundary!.y));
    expect(box!.x + box!.width).toBeLessThanOrEqual(
      Math.min(viewport.width, boundary!.x + boundary!.width) + 0.5,
    );
    expect(box!.y + box!.height).toBeLessThanOrEqual(
      Math.min(viewport.height, boundary!.y + boundary!.height) + 0.5,
    );
    expect(
      await control.evaluate((element) => {
        const rect = element.getBoundingClientRect();
        return element.contains(
          document.elementFromPoint(
            rect.x + rect.width / 2,
            rect.y + rect.height / 2,
          ),
        );
      }),
    ).toBe(true);
    const localBox = {
      ...box!,
      x: box!.x - boundary!.x,
      y: box!.y - boundary!.y,
    };
    for (const sibling of boxes) {
      const intersectionWidth =
        Math.min(localBox.x + localBox.width, sibling.x + sibling.width) -
        Math.max(localBox.x, sibling.x);
      const intersectionHeight =
        Math.min(localBox.y + localBox.height, sibling.y + sibling.height) -
        Math.max(localBox.y, sibling.y);
      expect(intersectionWidth <= 0.5 || intersectionHeight <= 0.5).toBe(true);
    }
    boxes.push(localBox);
  }
  expect(boxes.length).toBeGreaterThan(0);
}

async function expectSiblings(owner: Locator) {
  const geometry = await owner.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return {
      boundary: {
        left: rect.left,
        right: rect.right,
        top: rect.top,
        bottom: rect.bottom,
      },
      children: [...element.children]
        .filter((child) => child.checkVisibility())
        .map((child) => {
          const box = child.getBoundingClientRect();
          return {
            left: box.left,
            right: box.right,
            top: box.top,
            bottom: box.bottom,
          };
        }),
    };
  });
  for (const [index, child] of geometry.children.entries()) {
    expect(child.left).toBeGreaterThanOrEqual(geometry.boundary.left);
    expect(child.right).toBeLessThanOrEqual(geometry.boundary.right + 0.5);
    expect(child.top).toBeGreaterThanOrEqual(geometry.boundary.top);
    expect(child.bottom).toBeLessThanOrEqual(geometry.boundary.bottom + 0.5);
    for (const sibling of geometry.children.slice(index + 1)) {
      expect(
        Math.min(child.right, sibling.right) -
          Math.max(child.left, sibling.left) <=
          0.5 ||
          Math.min(child.bottom, sibling.bottom) -
            Math.max(child.top, sibling.top) <=
            0.5,
      ).toBe(true);
    }
  }
}

async function expectComposerControls(
  composer: Locator,
  viewport: { width: number; height: number },
) {
  const remove = composer.locator(".composer__attachment-remove");
  await expect(remove).toHaveCount(2);
  await expect(
    composer.getByRole("button", { name: /^Remove Page/ }),
  ).toBeEnabled();
  await expect(
    composer.getByRole("button", { name: /^Remove Image/ }),
  ).toBeEnabled();
  await expectControls(
    composer,
    composer.locator('button, [tabindex="0"]'),
    viewport,
  );
}

async function expectOverlay(
  panel: Locator,
  viewport: { width: number; height: number },
) {
  await expect(panel).toBeVisible();
  await expect
    .poll(async () => {
      const box = await panel.boundingBox();
      return Boolean(
        box &&
        box.x >= 8 &&
        box.y >= 8 &&
        box.x + box.width <= viewport.width - 7.5 &&
        box.y + box.height <= viewport.height - 7.5,
      );
    })
    .toBe(true);
  const actionable = panel
    .locator('button:enabled, input:not(:disabled), [tabindex="0"]')
    .first();
  if (await actionable.count()) {
    await actionable.scrollIntoViewIfNeeded();
    expect(
      await actionable.evaluate((element) => {
        const rect = element.getBoundingClientRect();
        return element.contains(
          document.elementFromPoint(
            rect.x + rect.width / 2,
            rect.y + rect.height / 2,
          ),
        );
      }),
    ).toBe(true);
  }
}

for (const profile of profiles) {
  for (const viewport of viewports) {
    for (const two of [false, true]) {
      for (const forcedColors of ["none", "active"] as const) {
        test(`${profile.name} chat geometry ${viewport.width}x${viewport.height}, ${two ? "two selected" : "single"}, ${forcedColors}`, async ({
          page,
        }) => {
          await page.setViewportSize(viewport);
          await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
          await prepare(page, profile, two, true);
          const panes = page.locator(".agent-chat-pane");
          await expect(panes).toHaveCount(
            two && viewport.width === 1888 ? 2 : 1,
          );
          const header = page.locator(".agent-task-header");
          await expectControls(header, header.locator("button"), viewport);
          await expectSiblings(header);
          for (const split of two && viewport.width === 1888
            ? [null, "Home", "End"]
            : [null]) {
            if (split) {
              await page
                .getByRole("separator", { name: "Resize chat panes" })
                .focus();
              await page.keyboard.press(split);
              await expect(
                page.getByRole("separator", { name: "Resize chat panes" }),
              ).toHaveAttribute(
                "aria-valuenow",
                split === "Home" ? "30" : "70",
              );
            }
            for (const pane of await panes.all()) {
              const composer = pane.locator("form.composer");
              await expectComposerControls(composer, viewport);
              await expect(
                composer.getByRole("button", { name: "Harness", exact: true }),
              ).toBeVisible();
              await expect(
                composer.getByRole("button", { name: "Model", exact: true }),
              ).toBeVisible();
              await expect(
                composer.getByRole("button", {
                  name: "Access options",
                  exact: true,
                }),
              ).toBeVisible();
              await expect(composer.getByRole("progressbar")).toHaveCount(
                profile.context ? 1 : 0,
              );
              await expect(composer.locator(".chat-goal-strip")).toHaveCount(
                profile.goal ? 1 : 0,
              );
              await expectSiblings(composer.locator(".composer__queued-row"));
              if (profile.goal)
                await expectSiblings(composer.locator(".chat-goal-strip__row"));
              const queue = composer.getByRole("button", {
                name: /^More options for queued message:/,
              });
              await queue.click();
              await expectOverlay(
                page.getByRole("menu", { name: "Queued message options" }),
                viewport,
              );
              await page.keyboard.press("Escape");
              await expect(queue).toBeFocused();
            }
          }
          await header
            .getByRole("button", { name: "Show chat resources" })
            .click();
          await expectOverlay(
            page.getByRole("dialog", { name: "Chat resources", exact: true }),
            viewport,
          );
          await page.keyboard.press("Escape");
          await expect(
            header.getByRole("button", { name: "Show chat resources" }),
          ).toBeFocused();
          const summary = panes.first().locator(".acp-file-changes").first();
          await revealWorkedContent(summary);
          const disclosure = summary.locator(".acp-file-changes__header");
          if ((await disclosure.getAttribute("aria-expanded")) === "false")
            await disclosure.click();
          await summary.scrollIntoViewIfNeeded();
          await expectControls(
            summary,
            summary.locator("button"),
            viewport,
            true,
          );
        });
      }
    }
  }
}

for (const profile of profiles) {
  for (const two of [false, true]) {
    for (const forcedColors of ["none", "active"] as const) {
      test(`${profile.name} ready compact controls at every viewport and split, ${two ? "two selected" : "single"}, ${forcedColors}`, async ({
        page,
      }) => {
        await page.setViewportSize(viewports[2]);
        await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
        await prepare(page, profile, two, false);
        for (const viewport of viewports) {
          await page.setViewportSize(viewport);
          const panes = page.locator(".agent-chat-pane");
          await expect(panes).toHaveCount(
            two && viewport.width === 1888 ? 2 : 1,
          );
          for (const split of two && viewport.width === 1888
            ? [null, "Home", "End"]
            : [null]) {
            if (split) {
              const splitter = page.getByRole("separator", {
                name: "Resize chat panes",
              });
              await splitter.focus();
              await page.keyboard.press(split);
              await expect(splitter).toHaveAttribute(
                "aria-valuenow",
                split === "Home" ? "30" : "70",
              );
            }
            for (const pane of await panes.all()) {
              await expectComposerControls(
                pane.locator("form.composer"),
                viewport,
              );
            }
          }
        }
      });
    }
  }
}

for (const forcedColors of ["none", "active"] as const) {
  test(`options follow the anchor after Tab to the splitter and Home/End (${forcedColors})`, async ({
    page,
  }) => {
    const viewport = viewports[2];
    await page.setViewportSize(viewport);
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await prepare(page, profiles[0], true, false);
    const trigger = page.locator("form.composer").nth(1).getByRole("button", {
      name: "Composer options",
      exact: true,
    });
    const splitter = page.getByRole("separator", { name: "Resize chat panes" });
    await trigger.click();
    const panel = page.getByRole("dialog", { name: "Composer options menu" });
    await expect(panel).toBeVisible();
    const initialAnchor = await trigger.boundingBox();
    for (let step = 0; step < 60; step++) {
      if (
        await splitter.evaluate((element) => element === document.activeElement)
      )
        break;
      await page.keyboard.press("Tab");
    }
    await expect(splitter).toBeFocused();
    for (const key of ["Home", "End"]) {
      await page.keyboard.press(key);
      await expect(splitter).toHaveAttribute(
        "aria-valuenow",
        key === "Home" ? "30" : "70",
      );
      await expect(panel).toBeVisible();
      await expect
        .poll(async () => {
          const anchor = await trigger.boundingBox();
          const surface = await panel.boundingBox();
          return surface!.x + surface!.width - (anchor!.x + anchor!.width);
        })
        .toBeCloseTo(0, 0);
      const anchor = await trigger.boundingBox();
      expect(anchor!.width).toBe(initialAnchor!.width);
      expect(anchor!.height).toBe(initialAnchor!.height);
      expect(anchor!.x).not.toBe(initialAnchor!.x);
      await expectOverlay(panel, viewport);
      await expect(splitter).toBeFocused();
    }
  });
}

for (const profile of profiles) {
  for (const forcedColors of ["none", "active"] as const) {
    test(`${profile.name} empty chat pickers remain hit-testable and viewport-contained through resizing (${forcedColors})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1888, height: 1096 });
      await page.emulateMedia({ forcedColors, reducedMotion: "no-preference" });
      await prepare(page, profile, true, false);
      const composer = page.locator("form.composer").first();
      const harness = composer.getByRole("button", {
        name: "Harness",
        exact: true,
      });
      const model = composer.getByRole("button", {
        name: "Model",
        exact: true,
      });
      const harnessBox = await harness.boundingBox();
      const modelBox = await model.boundingBox();
      expect(harnessBox!.x + harnessBox!.width).toBeLessThanOrEqual(
        modelBox!.x,
      );
      for (const name of [
        "Harness",
        "Model",
        ...(profile.effort ? ["Thinking level"] : []),
        "Composer options",
        "Access options",
      ]) {
        const trigger = composer.getByRole("button", { name, exact: true });
        await trigger.focus();
        await page.keyboard.press("Enter");
        const panel = page.locator(".menu-popover").filter({ visible: true });
        await expect(panel).toHaveCount(1);
        if (name === "Composer options" || name === "Access options") {
          const first = panel.locator("button:enabled").first();
          const second = panel.locator("button:enabled").nth(1);
          await expect(first).toBeFocused();
          await page.keyboard.press("Tab");
          await expect(second).toBeFocused();
          await page.keyboard.press("Shift+Tab");
          await expect(first).toBeFocused();
        }
        for (const viewport of [
          viewports[2],
          viewports[1],
          viewports[0],
          viewports[3],
          viewports[2],
        ]) {
          await page.setViewportSize(viewport);
          await expectOverlay(panel, viewport);
          await expect(composer.getByRole("textbox")).toHaveValue(
            "Draft survives resizing",
          );
        }
        await page.keyboard.press("Escape");
        await expect(
          composer.getByRole("button", {
            name,
            exact: true,
          }),
        ).toBeFocused();
      }
      await composer
        .getByRole("button", { name: "Composer options", exact: true })
        .click();
      const options = page.getByRole("dialog", {
        name: "Composer options menu",
      });
      await options.getByRole("button", { name: "Permission mode" }).click();
      const picker = options.locator(".searchable-picker__popover");
      await expectOverlay(picker, viewports[2]);
      await page.setViewportSize(viewports[3]);
      await expectOverlay(options, viewports[3]);
      await expectOverlay(picker, viewports[3]);
      const selected = picker.getByRole("option").first();
      await selected.click();
      await expect(picker).toHaveCount(0);
      await options.getByRole("button", { name: "Permission mode" }).focus();
      await page.keyboard.press("Enter");
      await expect(
        picker.getByRole("combobox", { name: "Permission mode" }),
      ).toBeFocused();
      await page.keyboard.press("Escape");
      await expect(
        options.getByRole("button", { name: "Permission mode" }),
      ).toBeFocused();
      await expect(options).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(options).toHaveCount(0);
      await expect(
        composer.getByRole("button", { name: "Composer options", exact: true }),
      ).toBeFocused();
      await page.setViewportSize(viewports[2]);
      await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
      for (const field of await page.locator("form.composer textarea").all()) {
        await expect(field).toHaveValue("Draft survives resizing");
      }
    });
  }
}
