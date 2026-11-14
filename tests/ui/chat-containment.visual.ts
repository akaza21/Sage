import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  waitForAnimations,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";
import { openComposerOptions, revealWorkedContent } from "./agentVisualHelpers";
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
  await page.addScriptTag({
    type: "module",
    content: `
      import { installChatContainmentFixture } from "/tests/ui/fixtures/chatContainmentFixture.ts";
      installChatContainmentFixture(${JSON.stringify(profile)}, ${JSON.stringify(live)});
    `,
  });
  await expect(
    page
      .locator("form.composer")
      .first()
      .getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("Draft survives resizing");
  // The queued rows are a lazy chunk that loads with the first queued
  // message (lazyParts.tsx), so geometry waits for the live queue to mount.
  if (live)
    await expect(
      page.locator("form.composer .composer__queued-row").first(),
    ).toBeVisible();
}

async function expectControls(
  owner: Locator,
  controls: Locator,
  viewport: { width: number; height: number },
  scrollControls = false,
  // A queue that scrolls as one block keeps its controls' local offsets.
  checkOverlap = !scrollControls,
) {
  // One page round trip measures every control; a round trip per control and
  // property made these loops slow enough to time out under CPU load.
  const ownerHandle = await owner.elementHandle();
  const measured = await controls.evaluateAll(
    (elements, { root, scroll }) =>
      elements.flatMap((element) => {
        const visible = () => {
          const rect = element.getBoundingClientRect();
          return (
            rect.width > 0 &&
            rect.height > 0 &&
            getComputedStyle(element).visibility !== "hidden"
          );
        };
        if (!visible()) return [];
        if (scroll)
          (
            element as Element & {
              scrollIntoViewIfNeeded(center: boolean): void;
            }
          ).scrollIntoViewIfNeeded(true);
        const boundary = root!.getBoundingClientRect();
        const rect = element.getBoundingClientRect();
        return [
          {
            plus: element.classList.contains("composer__plus"),
            boundary: {
              x: boundary.x,
              y: boundary.y,
              width: boundary.width,
              height: boundary.height,
            },
            box: {
              x: rect.x,
              y: rect.y,
              width: rect.width,
              height: rect.height,
            },
            hit: element.contains(
              document.elementFromPoint(
                rect.x + rect.width / 2,
                rect.y + rect.height / 2,
              ),
            ),
          },
        ];
      }),
    { root: ownerHandle, scroll: scrollControls },
  );
  await ownerHandle?.dispose();
  const boxes: { x: number; y: number; width: number; height: number }[] = [];
  for (const { plus, boundary, box, hit } of measured) {
    // The plus is an exact 24px target at every composer width.
    if (plus) expect(box.width).toBe(24);
    else expect(box.width).toBeGreaterThanOrEqual(24);
    expect(box.height).toBeGreaterThanOrEqual(24);
    expect(box.x).toBeGreaterThanOrEqual(Math.max(0, boundary.x));
    expect(box.y).toBeGreaterThanOrEqual(Math.max(0, boundary.y));
    expect(box.x + box.width).toBeLessThanOrEqual(
      Math.min(viewport.width, boundary.x + boundary.width) + 0.5,
    );
    expect(box.y + box.height).toBeLessThanOrEqual(
      Math.min(viewport.height, boundary.y + boundary.height) + 0.5,
    );
    expect(hit).toBe(true);
    const localBox = {
      ...box,
      x: box.x - boundary.x,
      y: box.y - boundary.y,
    };
    if (checkOverlap) {
      for (const sibling of boxes) {
        const intersectionWidth =
          Math.min(localBox.x + localBox.width, sibling.x + sibling.width) -
          Math.max(localBox.x, sibling.x);
        const intersectionHeight =
          Math.min(localBox.y + localBox.height, sibling.y + sibling.height) -
          Math.max(localBox.y, sibling.y);
        expect(intersectionWidth <= 0.5 || intersectionHeight <= 0.5).toBe(
          true,
        );
      }
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
    composer.locator(
      'button:not(.composer__queued button), [tabindex="0"]:not(.composer__queued [tabindex="0"])',
    ),
    viewport,
    true,
  );
  const queue = composer.locator(".composer__queued");
  if (await queue.count()) {
    const box = await queue.boundingBox();
    const owner = await composer.boundingBox();
    const toolbar = await composer.locator(".composer__toolbar").boundingBox();
    expect(box!.y).toBeGreaterThanOrEqual(owner!.y);
    expect(box!.y + box!.height).toBeLessThanOrEqual(toolbar!.y);
    expect(box!.x).toBeGreaterThanOrEqual(owner!.x);
    expect(box!.x + box!.width).toBeLessThanOrEqual(owner!.x + owner!.width);
    // Every row's controls remain reachable in the bounded queue viewport.
    await expectControls(
      queue,
      queue.locator('button, [tabindex="0"]'),
      viewport,
      true,
      true,
    );
  }
}

async function expectOverlay(
  panel: Locator,
  viewport: { width: number; height: number },
) {
  await expect(panel).toBeVisible();
  await waitForAnimations(panel, { subtree: false });
  await expect
    .poll(() =>
      panel.evaluate((element, { width, height }) => {
        const box = element.getBoundingClientRect();
        if (
          box.width === 0 ||
          box.height === 0 ||
          box.x < 8 ||
          box.y < 8 ||
          box.right > width - 7.5 ||
          box.bottom > height - 7.5
        )
          return "outside the viewport";
        const actionable = element.querySelector(
          'button:enabled, input:not(:disabled), [tabindex="0"]',
        );
        if (!actionable) return "contained";
        (
          actionable as Element & {
            scrollIntoViewIfNeeded(center: boolean): void;
          }
        ).scrollIntoViewIfNeeded(true);
        const rect = actionable.getBoundingClientRect();
        return actionable.contains(
          document.elementFromPoint(
            rect.x + rect.width / 2,
            rect.y + rect.height / 2,
          ),
        )
          ? "contained"
          : "covered";
      }, viewport),
    )
    .toBe("contained");
}

async function expectNestedPlacement(picker: Locator, parent: Locator) {
  await waitForAnimations(parent, { subtree: false });
  await waitForAnimations(picker);
  const child = await picker.boundingBox();
  const owner = await parent.boundingBox();
  expect(
    child!.x >= owner!.x + owner!.width ||
      child!.x + child!.width <= owner!.x ||
      child!.y >= owner!.y + owner!.height ||
      child!.y + child!.height <= owner!.y,
  ).toBe(true);
}

async function installModeActions(page: Page, chatIndex = 0) {
  await page.evaluate(async (index) => {
    const fixture =
      await import("/tests/ui/fixtures/surfaces/composerActionsVisualFixture.ts");
    fixture.installVisualComposerActionsFixture(
      [
        { id: "review", name: "Review changes" },
        { id: "apply", name: "Apply changes" },
      ],
      index,
    );
  }, chatIndex);
}

for (const forcedColors of ["none", "active"] as const) {
  test(`single-surface Session mode choices and Project stay reachable at 480x560 (${forcedColors})`, async ({
    page,
  }) => {
    await page.setViewportSize(viewports[3]);
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await openScenario(page, "agent-session-mode-open", "agent");
    await openComposerOptions(page);
    const options = page.getByRole("menu", { name: "Composer options menu" });
    const picker = options;
    await expectOverlay(picker, viewports[3]);
    // Project lives in the same menu once the chat has started.
    const project = options.getByRole("menuitem", {
      name: "Work in a project",
      exact: true,
    });
    await expect(project).toBeVisible();
    const last = picker.getByRole("menuitemradio").last();
    await last.scrollIntoViewIfNeeded();
    expect(
      await last.evaluate((element) => {
        const rect = element.getBoundingClientRect();
        return element.contains(
          document.elementFromPoint(
            rect.x + rect.width / 2,
            rect.y + rect.height / 2,
          ),
        );
      }),
    ).toBe(true);
    await last.click();
    await expect(picker).toHaveCount(0);
    await expect(options).toHaveCount(0);
    await openComposerOptions(page);
    await project.click();
    const projectPicker = page.locator(".searchable-picker__popover");
    await expectOverlay(projectPicker, viewports[3]);
    await page.keyboard.press("Escape");
    await expect(projectPicker).toHaveCount(0);
  });
}

test("single-pane composer options stay inside the wide dock", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await prepare(page, profiles[0], false, false);
  await installModeActions(page);
  const composer = page.locator("form.composer");
  await composer
    .getByRole("button", { name: "Composer options", exact: true })
    .click();
  const options = page.getByRole("menu", { name: "Composer options menu" });
  await expectOverlay(options, { width: 1280, height: 1000 });
  const bounds = await page.locator(".chat-dock").boundingBox();
  const panel = await options.boundingBox();
  expect(panel!.x).toBeGreaterThanOrEqual(bounds!.x);
  expect(panel!.x + panel!.width).toBeLessThanOrEqual(
    bounds!.x + bounds!.width,
  );
});

test("expanded narrow file changes keep names and totals on one row", async ({
  page,
}) => {
  await page.setViewportSize(viewports[0]);
  await prepare(page, profiles[0], false, true);
  const summary = page.locator(".acp-file-changes").first();
  await revealWorkedContent(summary);
  const disclosure = summary.locator(".acp-file-changes__header");
  if ((await disclosure.getAttribute("aria-expanded")) === "false")
    await disclosure.click();
  await waitForAnimations(summary);
  const rows = summary.locator(".chat-file-chip");
  expect(await rows.count()).toBeGreaterThan(0);
  for (const row of await rows.all()) {
    const name = row.locator(".chat-file-chip__name");
    await expect(name).toHaveCSS("text-align", "left");
    await expect(name).toHaveCSS("text-overflow", "ellipsis");
    const nameBox = await name.boundingBox();
    expect(nameBox!.width).toBeGreaterThanOrEqual(48);
    for (const detail of await row
      .locator(".chat-file-chip__root, .tab__count")
      .all()) {
      if ((await detail.getAttribute("class")) === "chat-file-chip__root") {
        const chatWidth = (await page
          .locator(".agent-chat-pane")
          .first()
          .boundingBox())!.width;
        if (chatWidth <= 400) await expect(detail).toBeHidden();
        else await expect(detail).toBeVisible();
        continue;
      }
      await expect(detail).toBeVisible();
      const detailBox = await detail.boundingBox();
      expect(Math.abs(detailBox!.y - nameBox!.y)).toBeLessThanOrEqual(2);
      expect(detailBox!.x).toBeGreaterThan(nameBox!.x);
    }
    expect((await row.boundingBox())!.height).toBeLessThanOrEqual(28);
  }
});

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
          await expect(panes).toHaveCount(1);
          const header = page.locator(".agent-task-header");
          await expectControls(header, header.locator("button"), viewport);
          await expectSiblings(header);
          for (const split of two && viewport.width === 1888
            ? [null, "Home", "End"]
            : [null]) {
            if (split) {
              await page
                .getByRole("separator", {
                  name: "Resize project and chat rail",
                })
                .focus();
              await page.keyboard.press(split);
              await expect(
                page.getByRole("separator", {
                  name: "Resize project and chat rail",
                }),
              ).toHaveAttribute(
                "aria-valuenow",
                split === "Home" ? "180" : "460",
              );
            }
            for (const pane of await panes.all()) {
              const composer = pane.locator("form.composer");
              const paneBox = await pane.boundingBox();
              const composerBox = await composer.boundingBox();
              expect(composerBox!.y + composerBox!.height).toBeLessThanOrEqual(
                paneBox!.y + paneBox!.height + 0.5,
              );
              expect(
                (await pane.locator(".message-list").boundingBox())!.height,
              ).toBeGreaterThanOrEqual(120);
              if (viewport.width === 480 && viewport.height === 560) {
                const body = composer.locator(".composer__body");
                await expect(body).toHaveCSS("overflow-y", "auto");
                expect((await body.boundingBox())!.height).toBeLessThanOrEqual(
                  composerBox!.height,
                );
                if (
                  await body.evaluate(
                    (element) => element.scrollHeight > element.clientHeight,
                  )
                ) {
                  await body.evaluate((element) => {
                    element.scrollTop = element.scrollHeight;
                  });
                  expect(
                    await body.evaluate((element) => element.scrollTop),
                  ).toBeGreaterThan(0);
                }
              }
              await expectControls(
                composer,
                composer.locator(".composer__toolbar button"),
                viewport,
              );
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
              // Like Codex refs 36/37, the context meter is the first thing a
              // tight composer drops: it never outlasts a control label.
              // (The width sweep in composer-compaction checks it returns.)
              const labelsCompacted = await composer.evaluate((element) =>
                [
                  ".composer__access-text",
                  ".composer__harness .searchable-picker__trigger-label",
                  ".composer__model-label",
                  ".composer__effort-picker[aria-controls] .composer__effort-label",
                ].some((selector) => {
                  const label = element.querySelector(selector);
                  return (
                    label !== null && label.getBoundingClientRect().width === 0
                  );
                }),
              );
              if (!profile.context || labelsCompacted)
                await expect(composer.getByRole("progressbar")).toHaveCount(0);
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
          await expect(panes).toHaveCount(1);
          for (const split of two && viewport.width === 1888
            ? [null, "Home", "End"]
            : [null]) {
            if (split) {
              const splitter = page.getByRole("separator", {
                name: "Resize project and chat rail",
              });
              await splitter.focus();
              await page.keyboard.press(split);
              await expect(splitter).toHaveAttribute(
                "aria-valuenow",
                split === "Home" ? "180" : "460",
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
  test(`queue actions preserve native Tab and Shift+Tab focus restoration (${forcedColors})`, async ({
    page,
  }) => {
    await page.setViewportSize(viewports[0]);
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await prepare(page, profiles[0], false, true);
    const trigger = page
      .locator("form.composer")
      .getByRole("button", { name: /^More options for queued message:/ });
    for (const key of ["Tab", "Shift+Tab"]) {
      await trigger.focus();
      await page.keyboard.press("Enter");
      const menu = page.getByRole("menu", { name: "Queued message options" });
      await expect(menu).toBeVisible();
      await expect(menu.getByRole("menuitem")).toBeFocused();
      await page.keyboard.press(key);
      await expect(menu).toHaveCount(0);
      await expect(trigger).toBeFocused();
    }
  });

  test(`options return boundary Tab focus and follow navigation splitter movement (${forcedColors})`, async ({
    page,
  }) => {
    const viewport = viewports[2];
    await page.setViewportSize(viewport);
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await prepare(page, profiles[0], true, false);
    await installModeActions(page, 0);
    const trigger = page.locator("form.composer").first().getByRole("button", {
      name: "Composer options",
      exact: true,
    });
    const splitter = page.getByRole("separator", {
      name: "Resize project and chat rail",
    });
    await trigger.click();
    const panel = page.getByRole("menu", { name: "Composer options menu" });
    await expect(panel).toBeVisible();
    await panel.locator("button:enabled").last().focus();
    await page.keyboard.press("Tab");
    await expect(panel).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await trigger.click();
    await expect(panel).toBeVisible();
    await waitForAnimations(panel);
    const initialAnchor = await trigger.boundingBox();
    // The leading options trigger sits at the composer's left edge, so the
    // shared placement clamps the panel to the composer instead of
    // right-aligning it; following the anchor keeps this offset constant.
    const initialOffset = (await panel.boundingBox())!.x - initialAnchor!.x;
    await splitter.focus();
    await expect(splitter).toBeFocused();
    for (const key of ["Home", "End"]) {
      await page.keyboard.press(key);
      await expect(splitter).toHaveAttribute(
        "aria-valuenow",
        key === "Home" ? "180" : "460",
      );
      await expect(panel).toBeVisible();
      await expect
        .poll(async () => {
          const anchor = await trigger.boundingBox();
          const surface = await panel.boundingBox();
          return surface!.x - anchor!.x;
        })
        .toBeCloseTo(initialOffset, 0);
      const anchor = await trigger.boundingBox();
      // A composer at or below 520px uses the intended compact 24px target.
      expect([initialAnchor!.width, 24]).toContain(anchor!.width);
      expect(anchor!.height).toBe(initialAnchor!.height);
      expect(anchor!.x).not.toBe(initialAnchor!.x);
      await expectOverlay(panel, viewport);
      await expect(splitter).toBeFocused();
    }
  });
}

// One test per picker keeps each resize sweep short under CPU load.
const emptyChatPickers = (profile: ChatContainmentProfile) => [
  "Harness",
  "Model",
  ...(profile.effort ? ["Thinking level: High"] : []),
  // Add always offers Files and folders; its menu keyboard and resize
  // behaviour is covered with installModeActions.
  "Access options",
  "Project",
];

for (const profile of profiles) {
  for (const target of emptyChatPickers(profile)) {
    for (const forcedColors of ["none", "active"] as const) {
      test(`${profile.name} empty chat ${target} picker remains hit-testable and viewport-contained through resizing (${forcedColors})`, async ({
        page,
      }) => {
        await page.setViewportSize({ width: 1888, height: 1096 });
        await page.emulateMedia({
          forcedColors,
          reducedMotion: "no-preference",
        });
        await prepare(page, profile, true, false);
        const composer = page.locator("form.composer").first();
        if (target === "Harness") {
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
        }
        if (target !== "Project") {
          const name = target;
          const trigger = composer.getByRole("button", { name, exact: true });
          await trigger.focus();
          await page.keyboard.press("Enter");
          const panel = page.locator(".menu-popover").filter({ visible: true });
          await expect(panel).toHaveCount(1);
          if (name === "Access options") {
            const first = panel.getByRole("menuitemradio").first();
            const last = panel.getByRole("menuitemradio").last();
            await expect(first).toBeFocused();
            await page.keyboard.press("End");
            await expect(last).toBeFocused();
            await page.keyboard.press("Home");
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
          }
          await expect(
            composer.getByRole("combobox", { name: "Message agent" }),
          ).toHaveValue("Draft survives resizing");
          await page.keyboard.press("Escape");
          await expect(
            composer.getByRole("button", {
              name,
              exact: true,
            }),
          ).toBeFocused();
          return;
        }
        await expect(
          composer.getByRole("button", {
            name: "Composer options",
            exact: true,
          }),
        ).toBeEnabled();
        const options = page.getByRole("menu", {
          name: "Composer options menu",
        });
        // The fixture renames the project, so match the picker by its prefix.
        // This exercises the new-chat heading's picker; the location strip's
        // picker is covered by the project-chip geometry tests.
        const project = page
          .getByRole("region", { name: "New chat", exact: true })
          .first()
          .getByRole("button", { name: /^Project: / });
        await project.click();
        await expect(options).toHaveCount(0);
        const picker = page.locator(".searchable-picker__popover");
        await expectOverlay(picker, viewports[2]);
        await page.setViewportSize(viewports[1]);
        await expectOverlay(picker, viewports[1]);
        await expectNestedPlacement(picker, project);
        expect(
          await project.evaluate((element) => {
            const rect = element.getBoundingClientRect();
            return element.contains(
              document.elementFromPoint(
                rect.x + rect.width / 2,
                rect.y + rect.height / 2,
              ),
            );
          }),
        ).toBe(true);
        const selected = picker.getByRole("option").last();
        await selected.scrollIntoViewIfNeeded();
        expect(
          await selected.evaluate((element) => {
            const rect = element.getBoundingClientRect();
            return element.contains(
              document.elementFromPoint(
                rect.x + rect.width / 2,
                rect.y + rect.height / 2,
              ),
            );
          }),
        ).toBe(true);
        await selected.click();
        await expect(picker).toHaveCount(0);
        await project.focus();
        await page.keyboard.press("Enter");
        await expect(
          picker.getByRole("combobox", { name: /^Project: / }),
        ).toBeFocused();
        await page.keyboard.press("Escape");
        await expect(project).toBeFocused();
        await expect(options).toHaveCount(0);
        // The new-chat caption picker does not fold at 480; its open picker
        // stays contained across the resize.
        await page.keyboard.press("Enter");
        await expect(picker).toHaveCount(1);
        await page.setViewportSize(viewports[3]);
        await expect(project).toBeVisible();
        await expectOverlay(picker, viewports[3]);
        await page.keyboard.press("Escape");
        await expect(picker).toHaveCount(0);
        await page.setViewportSize(viewports[2]);
        await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
        for (const field of await page
          .locator("form.composer textarea")
          .all()) {
          await expect(field).toHaveValue("Draft survives resizing");
        }
      });
    }
  }
}

async function setComposerContentWidth(composer: Locator, width: number) {
  await composer.evaluate((element, width) => {
    const computed = getComputedStyle(element);
    element.style.width = `${width + Number.parseFloat(computed.borderLeftWidth) + Number.parseFloat(computed.borderRightWidth)}px`;
  }, width);
  expect(await composer.evaluate((element) => element.clientWidth)).toBe(width);
}

async function queuePrompts(page: Page, count: number) {
  await page.evaluate(async (count) => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    const template = {
      text: "Review " + "long-resource-and-project-name-".repeat(12),
      attachedContext: [],
      bindingId: state.harnesses[0].bindingId!,
      modelId: state.selectedModelId,
      status: "outcomeUnknown" as const,
    };
    useWorkbenchStore.setState({
      chatUiById: Object.fromEntries(
        Object.entries(state.chatUiById).map(([id, ui]) => [
          id,
          {
            ...ui,
            queuedPrompts: Array.from({ length: count }, (_, index) => ({
              ...template,
              id: `geometry-queue-${index}`,
            })),
          },
        ]),
      ),
    });
  }, count);
  await expect(
    page.locator("form.composer").first().locator(".composer__queued-row"),
  ).toHaveCount(count);
}

// Whether a flex row's visible children sit on one line or several.
async function rowLines(row: Locator) {
  return row.evaluate((element) => {
    const boxes = [...element.children]
      .map((child) => child.getBoundingClientRect())
      .filter((box) => box.width > 0);
    return boxes.some((box) =>
      boxes.some((other) => other.top >= box.bottom - 0.5),
    )
      ? "wrapped"
      : "single";
  });
}

test("the queue scrolls inside the composer flow and wraps its rows when narrow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await prepare(page, profiles[0], false, true);
  await queuePrompts(page, 12);
  const composer = page.locator("form.composer").first();
  const queue = composer.locator(".composer__queued");
  const row = composer.locator(".composer__queued-row").first();
  for (const width of [521, 520, 300]) {
    await setComposerContentWidth(composer, width);
    const at = `at ${width}px`;
    const tray = (await queue.boundingBox())!;
    const form = (await composer.boundingBox())!;
    expect(tray.y, at).toBeGreaterThanOrEqual(form.y);
    expect(tray.x, at).toBeGreaterThanOrEqual(form.x);
    expect(tray.x + tray.width, at).toBeLessThanOrEqual(form.x + form.width);
    expect(tray.height, at).toBeLessThanOrEqual(320);
    // Twelve long rows never grow the composer; the tray scrolls instead.
    expect(
      await queue.evaluate(
        (element) => element.scrollHeight > element.clientHeight,
      ),
      at,
    ).toBe(true);
    // Rows sit on the composer fill rather than painting their own.
    expect(
      await row.evaluate(
        (element) => getComputedStyle(element).backgroundColor,
      ),
      at,
    ).toBe("rgba(0, 0, 0, 0)");
    await expectSiblings(row);
  }
  // Narrow, the row wraps its state and actions onto a second line instead
  // of squeezing or overlapping them.
  expect(await rowLines(row)).toBe("wrapped");
});

test("queued rows stay in the composer below an edit summary that scrolls on its own", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await prepare(page, profiles[0], false, true);
  const composer = page.locator("form.composer").first();
  const transcript = page.locator(".message-list").first();
  const edits = page.locator(".chat-edit-summary-zone").first();
  await expect(edits).toBeVisible();
  await queuePrompts(page, 3);
  const queue = composer.locator(".composer__queued");
  await expect(queue).toBeVisible();
  await waitForAnimations(composer);
  const tray = (await queue.boundingBox())!;
  const form = (await composer.boundingBox())!;
  const summary = (await edits.boundingBox())!;
  const transcriptBox = (await transcript.boundingBox())!;
  // The queue is part of the composer, never floated over the transcript or
  // the edit summary.
  expect(tray.y).toBeGreaterThanOrEqual(form.y);
  expect(tray.y + tray.height).toBeLessThanOrEqual(form.y + form.height + 0.5);
  expect(summary.y + summary.height).toBeLessThanOrEqual(tray.y + 0.5);
  expect(transcriptBox.y + transcriptBox.height).toBeLessThanOrEqual(
    summary.y + 0.5,
  );
  const slot = await edits.evaluate((element) => {
    const style = getComputedStyle(element);
    return [style.overflowY, style.backgroundColor];
  });
  expect(slot).toEqual(["auto", "rgba(0, 0, 0, 0)"]);
});

test("a short pane caps the composer row at 60% and scrolls its upper content above a fixed toolbar", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 560 });
  await prepare(page, profiles[0], false, true);
  const surface = (await page.locator(".chat-surface").boundingBox())!;
  const composer = page.locator("form.composer").first();
  const form = (await composer.boundingBox())!;
  expect(form.height).toBeLessThanOrEqual(surface.height * 0.6 + 0.5);
  expect(
    (await page.locator(".message-list").boundingBox())!.height,
  ).toBeGreaterThanOrEqual(120);
  const body = composer.locator(".composer__body");
  expect(
    await body.evaluate(
      (element) => element.scrollHeight > element.clientHeight,
    ),
  ).toBe(true);
  const toolbarLocator = composer.locator(".composer__toolbar");
  const toolbar = (await toolbarLocator.boundingBox())!;
  const scroller = (await body.boundingBox())!;
  expect(scroller.y + scroller.height).toBeLessThanOrEqual(toolbar.y + 0.5);
  expect(toolbar.y + toolbar.height).toBeLessThanOrEqual(
    form.y + form.height + 0.5,
  );
  // The toolbar keeps its natural height instead of shrinking with the body.
  expect(toolbar.height).toBe(
    await toolbarLocator.evaluate((element) => element.scrollHeight),
  );
});

test("a harness notice takes its own row above the transcript and composer", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await prepare(page, profiles[0], false, false);
  const surface = page.locator(".chat-surface");
  const transcript = surface.locator(".message-list");
  const composer = surface.locator("form.composer");
  // This empty chat's location strip and lazy context rows mount once its
  // history settles; measure the composer they leave.
  await expect(
    composer.getByRole("group", { name: "Chat location" }),
  ).toBeVisible();
  await expect(
    composer.getByRole("list", { name: "Attached context" }),
  ).toBeVisible();
  await expect(composer.getByRole("alert")).toBeVisible();
  const before = (await transcript.boundingBox())!;
  const composerBefore = (await composer.boundingBox())!;
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    useWorkbenchStore.setState({
      harnesses: state.harnesses.map((harness) => ({
        ...harness,
        characterisation: "uncharacterised" as const,
      })),
    });
  });
  const notice = surface.locator(":scope > .build-notice");
  await expect(notice).toBeVisible();
  const noticeBox = (await notice.boundingBox())!;
  const transcriptBox = (await transcript.boundingBox())!;
  const composerBox = (await composer.boundingBox())!;
  expect(noticeBox.y + noticeBox.height).toBeLessThanOrEqual(
    transcriptBox.y + 0.5,
  );
  expect(transcriptBox.y + transcriptBox.height).toBeLessThanOrEqual(
    composerBox.y + 0.5,
  );
  expect(transcriptBox.height).toBeGreaterThanOrEqual(120);
  // The notice row comes out of the transcript, never the composer.
  expect(transcriptBox.height).toBeLessThan(before.height);
  expect(composerBox).toEqual(composerBefore);
});

test("the goal strip reflows with its composer container at 280px, not the window", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await prepare(page, profiles[0], false, true);
  const composer = page.locator("form.composer").first();
  const row = composer.locator(".chat-goal-strip__row");
  await setComposerContentWidth(composer, 281);
  await expect(row).toBeVisible();
  expect(await rowLines(row)).toBe("single");
  expect((await row.boundingBox())!.height).toBe(32);
  await setComposerContentWidth(composer, 280);
  expect(await rowLines(row)).toBe("wrapped");
  const wrapped = (await row.boundingBox())!;
  expect(wrapped.height).toBeGreaterThan(32);
  await expectSiblings(row);
});

test("attached chips stay on a centred row in a 360px-tall viewport", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 360 });
  await prepare(page, profiles[0], false, false);
  const row = page.locator("form.composer .composer__attachment-row").first();
  await row.scrollIntoViewIfNeeded();
  await expect(row).toBeVisible();
  const geometry = await row.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    // Centre of the content box: the row has a top border only.
    const top = box.top + Number.parseFloat(style.borderTopWidth);
    const centre = (top + box.bottom) / 2;
    return {
      chips: [...element.querySelectorAll(".composer__attachment")].map(
        (chip) => {
          const chipBox = chip.getBoundingClientRect();
          return chipBox.width > 0 &&
            chipBox.height > 0 &&
            chipBox.top >= box.top &&
            chipBox.bottom <= box.bottom
            ? "visible"
            : "clipped";
        },
      ),
      offCentre: [...element.children].map((child) => {
        const childBox = child.getBoundingClientRect();
        return Math.abs(childBox.top + childBox.height / 2 - centre);
      }),
    };
  });
  expect(geometry.chips).toEqual(["visible", "visible"]);
  for (const offset of geometry.offCentre)
    expect(offset).toBeLessThanOrEqual(1);
});
