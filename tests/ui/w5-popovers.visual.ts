import { expect, test, type Locator, type Page } from "@playwright/test";
import { expectCompactComposerTargets } from "./shared/composerGeometry";
import {
  waitForAnimations,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";
import { popoverProfiles } from "./fixtures/popoverProfiles";
import type {} from "./fixtures/popoverWorkbench";

async function openPopoverScenario(page: Page) {
  await openScenario(page, "agent-single", "agent");
  await expect(page.locator("form.composer").first()).toBeVisible();
  await expect(
    page.locator(".acp-chat-projection[data-chat-id]").first(),
  ).toBeVisible();
  await settleChatProjections(page);
  await page.evaluate(async () => {
    await import("/tests/ui/fixtures/popoverWorkbench.ts");
  });
}

async function subtleSelection(menu: Locator) {
  const selected = menu.locator(
    '[aria-checked="true"], [aria-selected="true"]',
  );
  await expect(selected).toHaveCount(1);
  await expect(selected.locator(".lucide-check")).toBeVisible();
  const color = await selected.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  );
  const channels = color.match(/[\d.]+/g)!.map(Number);
  expect(channels[0]).toBe(channels[1]);
  expect(channels[1]).toBe(channels[2]);
}

async function contained(menu: Locator, trigger: Locator, pane: Locator) {
  await expect(menu).toBeVisible();
  await waitForAnimations(menu, { subtree: false });
  await menu.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  const box = (await menu.boundingBox())!;
  const anchor = (await trigger.boundingBox())!;
  for (const summary of await pane.locator(".acp-file-changes").all())
    await waitForAnimations(summary);
  const edits = await pane
    .locator(".chat-edit-summary-zone:not(:empty)")
    .boundingBox();
  const owner = (await pane.boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(owner.x);
  expect(box.x + box.width).toBeLessThanOrEqual(owner.x + owner.width);
  expect(
    Math.min(box.x + box.width, anchor.x + anchor.width) -
      Math.max(box.x, anchor.x),
  ).toBeGreaterThan(0);
  expect(
    Math.min(
      Math.abs(box.y + box.height - anchor.y),
      Math.abs(box.y - anchor.y - anchor.height),
      edits ? Math.abs(box.y + box.height - edits.y) : Infinity,
    ),
  ).toBeLessThanOrEqual(9);
  if (edits)
    expect(
      box.y + box.height <= edits.y || box.y >= edits.y + edits.height,
    ).toBe(true);
  expect(
    await menu.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
}

for (const width of [1888, 720, 480]) {
  for (const profile of popoverProfiles) {
    test(`w5 ${profile.name} menus stay anchored inside persistent navigation at ${width}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openPopoverScenario(page);
      expect(await page.evaluate(() => document.compatMode)).toBe("CSS1Compat");
      await page.evaluate(
        (modes) => window.popoverWorkbench.access(modes),
        profile.access,
      );
      const pane = page.locator(".agent-chat-pane").first();
      const navigation = page.locator(".project-sidebar");
      await expect(navigation).toBeVisible();
      expect((await pane.boundingBox())!.x).toBeGreaterThanOrEqual(
        (await navigation.boundingBox())!.width,
      );
      const composer = pane.locator("form.composer");
      await page.evaluate(
        (modes) => window.popoverWorkbench.actions(modes),
        profile.modes,
      );
      const { harnessName, hasProjects } = await page.evaluate(async () => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        const state = useWorkbenchStore.getState();
        const chat = state.tabs.find((tab) => tab.kind === "chat")!;
        const id =
          state.chatUiById[chat.id]?.harnessId ?? state.selectedHarnessId;
        return {
          harnessName: state.harnesses.find((harness) => harness.id === id)!
            .name,
          hasProjects: state.projects.length > 0,
        };
      });
      // Add always offers Files and folders while the binding is usable.
      for (const [name, menuName, available] of [
        ["Access options", "Access mode", profile.access.length > 0],
        ["Composer options", "Composer options menu", true],
      ] as const) {
        const trigger = composer.getByRole("button", { name, exact: true });
        if (!available) {
          await expect(trigger).toHaveCount(0);
          await expect(
            page.getByRole("menu", { name: menuName, exact: true }),
          ).toHaveCount(0);
          continue;
        }
        await trigger.click();
        const menu = page.getByRole("menu", { name: menuName, exact: true });
        await contained(menu, trigger, pane);
        // Add lists every advertised mode, so the current one is the single
        // subtly marked selection.
        if (name === "Access options" || profile.modes.length > 0)
          await subtleSelection(menu);
        if (name === "Access options") {
          for (const nextWidth of [480, 720, 1888, width]) {
            await page.setViewportSize({ width: nextWidth, height: 1096 });
            await contained(menu, trigger, pane);
            await expect(navigation).toBeVisible();
          }
          expect((await menu.boundingBox())!.width).toBe(
            Math.min(
              420,
              (await pane.boundingBox())!.width,
              width - 8 - (await pane.boundingBox())!.x,
            ),
          );
        }
        if (name === "Access options") {
          await expect(
            menu.getByText(`How should ${harnessName} actions be approved?`, {
              exact: true,
            }),
          ).toBeVisible();
          await expect(menu.getByRole("menuitemradio")).toHaveCount(
            profile.access.length,
          );
          await expect(
            menu.locator(".composer-access-menu__option > svg:first-child"),
          ).toHaveCount(profile.access.length);
        }
        await page.keyboard.press("Escape");
        await expect(trigger).toBeFocused();
      }
      if (profile.effort)
        await page.evaluate(
          (effort) => window.popoverWorkbench.effort(effort),
          profile.effort,
        );
      else
        await expect(composer.locator(".composer__effort-picker")).toHaveCount(
          0,
        );
      const add = composer.getByRole("button", {
        name: "Composer options",
        exact: true,
      });
      if (width === 480) await expectCompactComposerTargets(composer);
      const compactEffort = await composer.evaluate(
        (element) => element.clientWidth <= 320,
      );
      // The effort control takes toolbar room, so it may have folded into
      // the Add menu since the options pass above.
      await add.click();
      await expect(
        page.getByRole("menuitem", { name: "Thinking level" }),
      ).toHaveCount(profile.effort && compactEffort ? 1 : 0);
      await expect(
        page.getByRole("menuitem", { name: "Work in a project", exact: true }),
      ).toHaveCount(hasProjects ? 1 : 0);
      await page.keyboard.press("Escape");
      for (const [name, selector] of [
        ...(profile.effort && !compactEffort
          ? [
              [
                `Thinking level: ${profile.effort.currentValue === "medium" ? "Medium" : "High"}`,
                ".composer__effort-menu",
              ],
            ]
          : []),
        ["Model", ".composer__model-menu"],
        ["Harness", ".composer__harness-menu"],
      ]) {
        const trigger = composer.getByRole("button", { name, exact: true });
        // Without an advertised access mode the composer is blocked
        // (CP-29d), so the model picker stays disabled rather than inventing one.
        if (name === "Model" && profile.access.length === 0) {
          await expect(trigger).toBeDisabled();
          continue;
        }
        await trigger.click();
        await contained(page.locator(selector), trigger, pane);
        if (selector === ".composer__effort-menu") {
          await expect(page.locator(".composer-effort__level")).toHaveText(
            profile.effort!.currentValue === "medium" ? "Medium" : "High",
          );
          const reset = page.getByRole("button", { name: "Reset effort" });
          if (profile.effort?.defaultValue) await expect(reset).toBeVisible();
          else await expect(reset).toHaveCount(0);
        } else await subtleSelection(page.locator(selector));
        await page.keyboard.press("Escape");
        await expect(trigger).toBeFocused();
      }
      const resources = page.getByRole("button", {
        name: "Show chat resources",
      });
      if (profile.subagents)
        await page.evaluate(
          (kind) => window.popoverWorkbench.subagents(kind),
          profile.subagents,
        );
      await resources.click();
      const overview = page.getByRole("dialog", { name: "Chat resources" });
      await contained(overview, resources, page.locator(".agent-chat-center"));
      if (profile.subagents) {
        const agents = overview.getByRole("region", { name: "Subagents" });
        await expect(
          agents.getByLabel("Recent subagents working: 1"),
        ).toBeVisible();
        await expect(
          agents.getByLabel("Recent subagents done: 1"),
        ).toBeVisible();
        expect(
          await agents.evaluate(
            (element) => element.scrollWidth <= element.clientWidth,
          ),
        ).toBe(true);
      } else
        await expect(
          overview.getByRole("region", { name: "Subagents" }),
        ).toHaveCount(0);
      await page.keyboard.press("Escape");
      await expect(resources).toBeFocused();
    });
  }
  test(`w5 nested picker preserves its originating chat bounds at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1096 });
    await openPopoverScenario(page);
    await page.evaluate(() => window.popoverWorkbench.nested());
    const trigger = page.getByRole("button", {
      name: "Nested fixture",
      exact: true,
    });
    await trigger.click();
    const parent = page.getByRole("dialog", { name: "Nested fixture parent" });
    const picker = parent.getByRole("button", {
      name: "Nested fixture picker",
    });
    await picker.click();
    const child = page.getByRole("listbox", {
      name: "Nested fixture picker",
      exact: true,
    });
    await expect(child.getByRole("option")).toHaveCount(2);
    const surface = child.locator("..");
    await surface.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    );
    await waitForAnimations(surface);
    const box = (await surface.boundingBox())!;
    const pane = (await page.locator(".agent-chat-pane").boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(pane.x);
    expect(box.x + box.width).toBeLessThanOrEqual(pane.x + pane.width);
    expect(box.y).toBeGreaterThanOrEqual(8);
    expect(box.y + box.height).toBeLessThanOrEqual(1088);
    await page.keyboard.press("Escape");
    await expect(picker).toBeFocused();
    await expect(parent).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();
  });
}

for (const height of [320, 400]) {
  test(`w5 header menus clamp vertically in a ${height}px viewport`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height });
    await openPopoverScenario(page);
    await page.evaluate(() => window.popoverWorkbench.subagents("children"));
    for (const [name, role, label] of [
      ["Show chat resources", "dialog", "Chat resources"],
      ["Chat actions for", "menu", "Chat tools"],
    ] as const) {
      const trigger = page
        .getByRole("button", { name: new RegExp(name) })
        .last();
      await trigger.click();
      const menu = page.getByRole(role, { name: new RegExp(label) });
      await expect(menu).toBeVisible();
      await menu.evaluate(
        () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
          ),
      );
      await waitForAnimations(menu, { subtree: false });
      const box = (await menu.boundingBox())!;
      expect(box.y).toBeGreaterThanOrEqual(8);
      expect(box.y + box.height).toBeLessThanOrEqual(height - 8);
      await page.keyboard.press("Escape");
      await expect(trigger).toBeFocused();
    }
  });
}
