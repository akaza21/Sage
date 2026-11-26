import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  expectComposerSweep,
  settledComposerFit,
  sweepComposerToolbar,
} from "./shared/composerGeometry";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

import {
  openSelectionScenario,
  dragAnswerPhrase,
} from "./shared/transcriptSelection";

for (const scoped of [false, true]) {
  test(`project dropdown matches the compact search menu with project set: ${scoped}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await installChrome(page, true);
    await page.evaluate(async (scoped) => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      await useWorkbenchStore
        .getState()
        .openNewChatDraft(
          scoped ? useWorkbenchStore.getState().project!.id : null,
        );
      const state = useWorkbenchStore.getState();
      const project =
        state.projects.find(
          (entry) => entry.id === state.newChatDraft?.projectId,
        ) ?? state.projects[0]!;
      useWorkbenchStore.setState({
        projects: [
          { ...project, name: "Sage" },
          ...["Alpha", "Reference sandbox", "Delta", "Go harness"].map(
            (name, index) => ({ ...project, id: `sample-${index}`, name }),
          ),
        ],
      });
    }, scoped);
    const location = page.getByRole("group", { name: "Chat location" });
    // The branch chip appears once the scoped project reads as a repository.
    await expect(
      location.getByRole("button", { name: /^Switch branch in / }),
    ).toHaveCount(scoped ? 1 : 0);
    const trigger = location.getByRole("button", {
      name: scoped ? "Project: Sage" : "Choose project",
      exact: true,
    });
    await trigger.click();
    const menu = page.locator(".searchable-picker__popover--menu");
    const input = menu.getByPlaceholder("Search projects");
    await expect(input).toBeFocused();
    await expect(menu).toHaveCSS("outline-style", "none");
    await expect(
      menu.getByRole("option", {
        name: "Don't work in a project",
        exact: true,
      }),
    ).toHaveCount(scoped ? 1 : 0);
    await expect(
      menu.locator(".searchable-picker__selected-check"),
    ).toHaveCount(scoped ? 1 : 0);
    await menu.getByRole("option", { name: "Alpha", exact: true }).hover();
    await expect(
      menu.getByRole("option", { name: "Alpha", exact: true }),
    ).toHaveCSS("box-shadow", "none");
    const heading = (await page
      .getByRole("heading", { name: "What should we work on?" })
      .boundingBox())!;
    const box = (await menu.boundingBox())!;
    expect(
      box.y >= heading.y + heading.height || box.y + box.height <= heading.y,
    ).toBe(true);
    await expect(page).toHaveScreenshot(
      `project-dropdown-${scoped ? "selected" : "none"}.png`,
    );
    await input.press("ArrowDown");
    await expect(menu).toHaveCSS("outline-style", "solid");
    await input.press("Escape");
    await expect(trigger).toBeFocused();
    await trigger.press("ArrowDown");
    await expect(menu).toBeVisible();
    await expect(menu).toHaveCSS("outline-style", "solid");
    await menu.getByPlaceholder("Search projects").press("Escape");
  });
}

async function openSplitComposerScenario(page: Page) {
  await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  await settleChatProjections(page);
}

/** The model chip shows the level and opens it at every toolbar fit. */
async function expectEffortReachable(composer: Locator, level: string) {
  await settledComposerFit(composer);
  const model = composer.getByRole("button", { name: "Model", exact: true });
  await expect(model.locator(".composer__model-effort")).toHaveText(level);
  await model.click();
  await expect(
    composer
      .page()
      .getByRole("dialog", { name: "Select model" })
      .getByRole("slider", { name: "Thinking level" }),
  ).toHaveAttribute("aria-valuetext", level);
  await composer.page().keyboard.press("Escape");
  await expect(model).toBeFocused();
}

async function expectResponsiveToolbar(composer: Locator) {
  await settledComposerFit(composer);
  await expect(
    composer.locator(".composer__toolbar .project-scope"),
  ).toHaveCount(0);
  const harness = composer.getByRole("button", {
    name: "Harness",
    exact: true,
  });
  const model = composer.getByLabel("Model", { exact: true });
  for (const trigger of [harness, model]) {
    await expect(trigger).toBeVisible();
    expect((await trigger.boundingBox())!.width).toBeGreaterThanOrEqual(24);
  }
  const harnessLabel = harness.locator(".searchable-picker__trigger-label");
  // The brand mark always stays whole inside the harness trigger.
  const harnessBox = (await harness.boundingBox())!;
  const markBox = (await harness.locator("[data-harness-icon]").boundingBox())!;
  expect(markBox.width).toBeGreaterThanOrEqual(12);
  expect(markBox.x).toBeGreaterThanOrEqual(harnessBox.x - 0.5);
  expect(markBox.x + markBox.width).toBeLessThanOrEqual(
    harnessBox.x + harnessBox.width + 0.5,
  );
  // The composer harness is its mark alone at every width.
  await expect(harnessLabel).toBeHidden();
  if (!(await model.locator(".composer__model-label").isVisible()))
    await expect(model.locator(".composer__model-icon")).toBeVisible();
  await expectToolbarGeometry(composer);
}

async function expectToolbarGeometry(composer: Locator) {
  const geometry = await composer.evaluate((element) => {
    const toolbar = element.querySelector(".composer__toolbar")!;
    const controls = element.querySelector(".composer__controls")!;
    const row = toolbar.getBoundingClientRect();
    const targets = [...controls.children]
      .map((child) => child.getBoundingClientRect())
      .filter((target) => target.width > 0 && target.height > 0);
    const buttons = [...toolbar.querySelectorAll("button")].filter(
      (button) =>
        button.getBoundingClientRect().width > 0 &&
        button.getBoundingClientRect().height > 0,
    );
    const bounds = buttons.map((button) => button.getBoundingClientRect());
    const controlsRow = controls.getBoundingClientRect();
    // The one turn control: the mic while idle and empty, else Send or Stop.
    const turn = toolbar
      .querySelector(".send-button, .composer__mic")!
      .getBoundingClientRect();
    // Only a row that cannot fit even as icons wraps, with the turn control
    // on a row of its own.
    const secondRow = turn.top >= controlsRow.bottom - 1;
    return {
      overflow: toolbar.scrollWidth > toolbar.clientWidth,
      disjoint: bounds.every((button, index) =>
        bounds
          .slice(index + 1)
          .every(
            (other) =>
              button.right <= other.left ||
              other.right <= button.left ||
              button.bottom <= other.top ||
              other.bottom <= button.top,
          ),
      ),
      hitTestable: buttons.every((button, index) => {
        const target = bounds[index];
        // Chromium hit-tests the rounded shape, so probe each corner just
        // inside its radius instead of the bounding-box corner.
        const radius = getComputedStyle(button).borderTopLeftRadius;
        const half = Math.min(target.width, target.height) / 2;
        const corner = Math.min(
          half,
          radius.endsWith("%")
            ? (Number.parseFloat(radius) / 100) * half * 2
            : Number.parseFloat(radius) || 0,
        );
        const inset = corner * (1 - Math.SQRT1_2) + 2;
        return [
          [target.left + target.width / 2, target.top + target.height / 2],
          [target.left + inset, target.top + inset],
          [target.right - inset, target.bottom - inset],
        ].every(([horizontal, vertical]) =>
          button.contains(document.elementFromPoint(horizontal, vertical)),
        );
      }),
      contained: [...targets, ...bounds].every(
        (target) =>
          target.left >= row.left &&
          target.right <= row.right &&
          target.top >= row.top &&
          target.bottom <= row.bottom,
      ),
      identityRow:
        secondRow ||
        targets.every(
          (target) =>
            Math.abs(
              target.top +
                target.height / 2 -
                controlsRow.top -
                controlsRow.height / 2,
            ) <= 1,
        ),
      responsiveRows: secondRow
        ? turn.top >= controlsRow.bottom
        : Math.abs(
            turn.top +
              turn.height / 2 -
              controlsRow.top -
              controlsRow.height / 2,
          ) <= 1,
    };
  });
  expect(geometry).toEqual({
    overflow: false,
    disjoint: true,
    hitTestable: true,
    contained: true,
    identityRow: true,
    responsiveRows: true,
  });
}

for (const width of [1888, 720, 480]) {
  for (const scenario of ["agent-single", "agent-dual"] as const) {
    test(`toolbar controls stay disjoint at ${width} in ${scenario}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, scenario, "agent");
      await settleChatProjections(page);
      for (const composer of await page.locator("form.composer").all()) {
        await expectResponsiveToolbar(composer);
      }
    });
  }
}

for (const contentWidth of [
  128, 184, 252, 259, 259.5, 260, 264, 280, 281, 320, 321, 400, 401,
]) {
  test(`toolbar remains hit-testable at ${contentWidth}px content width`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    const composer = page.locator("form.composer").first();
    await composer.evaluate((element, width) => {
      const computed = getComputedStyle(element);
      element.style.width = `${width + Number.parseFloat(computed.borderLeftWidth) + Number.parseFloat(computed.borderRightWidth)}px`;
    }, contentWidth);
    await expectResponsiveToolbar(composer);
    await page.evaluate(async () => {
      const fixture =
        await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
      fixture.installVisualComposerEffortFixture({
        levels: ["low", "medium", "extraordinarily high"],
        currentValue: "extraordinarily high",
      });
    });
    await expectEffortReachable(composer, "Extraordinarily high");
    await expectResponsiveToolbar(composer);
  });
}

for (const paneWidth of [560, 414, 400]) {
  test(`toolbar keeps its labels in a ${paneWidth}px pane`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.addStyleTag({
      content: `.agent-chat-panes { grid-template-columns: ${paneWidth}px; }`,
    });
    const composer = page.locator("form.composer").first();
    await expectResponsiveToolbar(composer);
  });
}

test("composer controls compact by their own container width, not the window", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  // The Goal fixture offers composer options, so the Add button is present.
  await openScenario(page, "agent-composer-goal", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer").first();
  const contentWidths = [580, 521, 520, 401, 400, 321, 320, 281, 280, 180];
  const borders = await composer.evaluate((element) => {
    const computed = getComputedStyle(element);
    return (
      Number.parseFloat(computed.borderLeftWidth) +
      Number.parseFloat(computed.borderRightWidth)
    );
  });
  const widths = contentWidths.map((width) => width + borders);
  // The window never changes, so whatever compacts follows the composer's
  // own width, and only as far as its measured row needs.
  expectComposerSweep(await sweepComposerToolbar(composer, widths));

  const access = composer.getByRole("button", { name: "Access options" });
  const harnessLabel = composer
    .getByRole("button", { name: "Harness", exact: true })
    .locator(".searchable-picker__trigger-label");
  const model = composer.getByRole("button", { name: "Model", exact: true });
  const modelLabel = model.locator(".composer__model-label");
  const modelIcon = model.locator(".composer__model-icon");
  for (const [index, width] of widths.entries()) {
    await composer.evaluate(
      (element, width) => (element.style.width = `${width}px`),
      width,
    );
    const at = `at ${contentWidths[index]}px`;
    expect(await composer.evaluate((element) => element.clientWidth), at).toBe(
      contentWidths[index],
    );
    await settledComposerFit(composer);
    const plus = (await composer
      .getByRole("button", { name: "Composer options", exact: true })
      .boundingBox())!;
    expect(plus.width, at).toBe(24);
    expect(plus.height, at).toBe(28);
    // The access control stays a target with or without its text.
    await expect(access, at).toBeVisible();
    expect((await access.boundingBox())!.width, at).toBeGreaterThanOrEqual(24);
    // The harness shows its mark alone at every width.
    await expect(harnessLabel, at).toBeHidden();
    await expect(composer.locator(".composer__shortcut"), at).toBeHidden();
    if (!(await modelLabel.isVisible())) {
      // The model name gives way to its icon, never to a bare chevron.
      await expect(modelIcon, at).toBeVisible();
      continue;
    }
    await expect(modelIcon, at).toBeHidden();
    // The label truncates inside its own box, before the chevron, without
    // clipping the trigger's leading edge.
    const geometry = await model.evaluate((trigger) => {
      const label = trigger.querySelector(".composer__model-label")!;
      const chevron = trigger.querySelector("svg:last-child")!;
      const box = trigger.getBoundingClientRect();
      const text = label.getBoundingClientRect();
      const glyph = chevron.getBoundingClientRect();
      return {
        labelStart: text.left - box.left,
        beforeChevron: text.right <= glyph.left + 0.5,
        chevron: [glyph.width, glyph.height],
        chevronInside: glyph.left >= box.left && glyph.right <= box.right + 0.5,
        ellipsis: getComputedStyle(label).textOverflow,
      };
    });
    expect(geometry.labelStart, at).toBeGreaterThanOrEqual(0);
    expect(geometry.beforeChevron, at).toBe(true);
    expect(geometry.chevron, at).toEqual([16, 16]);
    expect(geometry.chevronInside, at).toBe(true);
    expect(geometry.ellipsis, at).toBe("ellipsis");
  }
});

test.describe("toolbar 200% zoom reflow proxy", () => {
  test.use({ viewport: { width: 480, height: 360 }, deviceScaleFactor: 2 });
  test("keeps the narrow toolbar and editable textarea contained", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    expect(await page.evaluate(() => window.devicePixelRatio)).toBe(2);
    const composer = page.locator("form.composer").first();
    await expectResponsiveToolbar(composer);
    const textarea = composer.getByRole("combobox", { name: "Message agent" });
    await textarea.fill("Draft remains editable at 200% reflow");
    await expect(textarea).toHaveValue("Draft remains editable at 200% reflow");
    const field = (await textarea.boundingBox())!;
    const form = (await composer.boundingBox())!;
    const toolbar = (await composer
      .locator(".composer__toolbar")
      .boundingBox())!;
    // The 360 px viewport uses the compact 40–56 px textarea range.
    expect(field.height).toBeGreaterThanOrEqual(40);
    expect(field.height).toBeLessThanOrEqual(56);
    expect(field.y).toBeGreaterThanOrEqual(form.y);
    expect(field.y + field.height).toBeLessThanOrEqual(toolbar.y);
    expect(toolbar.y + toolbar.height).toBeLessThanOrEqual(
      form.y + form.height,
    );
    expect(form.y + form.height).toBeLessThanOrEqual(360);
  });
});

async function installChrome(page: Page, home: boolean) {
  await page.evaluate(async (home) => {
    const { installChatChromeFixture } =
      await import("/tests/ui/fixtures/chatChromeFixture.ts");
    installChatChromeFixture("codex", home);
  }, home);
}

test("each project chip shows its clear button right after the name", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await installChrome(page, true);
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    await useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id);
  });
  const home = page.getByRole("region", { name: "New chat", exact: true });
  for (const chip of [
    home.locator(".project-scope--caption"),
    page.getByRole("group", { name: "Chat location" }).locator(".project-scope"),
  ]) {
    await page.mouse.move(0, 0);
    const name = (await chip.locator(".project-scope__picker").boundingBox())!;
    await chip.hover();
    const clear = chip.getByRole("button", {
      name: "Remove project from this chat",
    });
    await expect(clear).toBeVisible();
    await expect(clear).toHaveAttribute(
      "title",
      "Remove project from this chat",
    );
    const box = (await clear.boundingBox())!;
    expect(box.x - (name.x + name.width)).toBeGreaterThanOrEqual(0);
    expect(box.x - (name.x + name.width)).toBeLessThanOrEqual(2);
    // Showing the button moves nothing.
    expect(await chip.locator(".project-scope__picker").boundingBox()).toEqual(
      name,
    );
  }
  const heading = (await home.getByRole("heading").boundingBox())!;
  const caption = (await home
    .locator(".project-scope__caption")
    .boundingBox())!;
  expect(
    Math.abs(
      caption.x + caption.width / 2 - (heading.x + heading.width / 2),
    ),
  ).toBeLessThanOrEqual(1);
});

test("a new chat's location sits on a tab joined to the top of its composer", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await installChrome(page, true);
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    await useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id);
  });

  const frame = page.locator(".chat-dock-frame").first();
  const composer = frame.locator("form.composer");
  const strip = frame.getByRole("group", { name: "Chat location" });
  await page.mouse.move(0, 0);
  await composer.getByRole("combobox", { name: "Message agent" }).focus();
  await expect(strip.locator(".project-scope__clear")).toBeHidden();

  await expect(strip).toContainText("This computer");
  const project = strip.getByRole("button", {
    name: "Project: Sage project with a long name",
    exact: true,
  });
  await expect(project).toBeVisible();
  await expect(project).toHaveAttribute("aria-haspopup", "listbox");
  const branch = strip.getByRole("button", { name: /^Switch branch in / });
  await expect(branch).toHaveText("feature/editor-polish");
  for (const width of [1888, 720, 480]) {
    await page.setViewportSize({ width, height: 1096 });
    await settledComposerFit(composer);
    const geometry = await frame.evaluate((element) => {
      const tab = element.querySelector(".composer-location")!;
      const strip = tab.getBoundingClientRect();
      const dock = element.querySelector(".chat-dock")!.getBoundingClientRect();
      const style = getComputedStyle(tab);
      const icons = [...tab.querySelectorAll("svg")]
        .filter(
          (icon) =>
            icon.checkVisibility() &&
            getComputedStyle(icon).visibility !== "hidden",
        )
        .map((icon) => {
          const box = icon.getBoundingClientRect();
          return [box.width, box.height];
        });
      const chips = [...tab.children].map((chip) =>
        (
          chip.querySelector(".project-scope__picker") ?? chip
        ).getBoundingClientRect(),
      );
      return {
        // Measured from the project trigger: the idle chip keeps no room for
        // its hidden clear button.
        gaps: chips
          .slice(1)
          .map((chip, index) => Math.round(chip.left - chips[index]!.right)),
        attached: Math.abs(dock.top - strip.bottom) <= 0.5,
        insets: [strip.left - dock.left, dock.right - strip.right],
        height: strip.height,
        corners: [
          style.borderTopLeftRadius,
          style.borderTopRightRadius,
          style.borderBottomLeftRadius,
          style.borderBottomRightRadius,
        ],
        icons,
      };
    });
    expect(geometry, `at ${width}px`).toEqual({
      gaps: [16, 16],
      attached: true,
      insets: [12, 12],
      height: 36,
      corners: ["12px", "12px", "0px", "0px"],
      icons: [
        [14, 14],
        [14, 14],
        [14, 14],
      ],
    });
    await expectResponsiveToolbar(composer);
    await expect(strip).toHaveScreenshot(
      `composer-location-branch-${width}.png`,
    );
  }
  // Hovering the project shows its clear button in the gap without moving
  // the branch chip.
  const branchLeft = async () => (await branch.boundingBox())!.x;
  const idleBranch = await branchLeft();
  await project.hover();
  const clear = strip.getByRole("button", {
    name: "Remove project from this chat",
  });
  await expect(clear).toBeVisible();
  expect(await branchLeft()).toBe(idleBranch);
  const clearBox = (await clear.boundingBox())!;
  expect(clearBox.x + clearBox.width).toBeLessThanOrEqual(idleBranch);
  await page.mouse.move(0, 0);
  await expect(project).toHaveCSS("font-size", "13px");
  await project.click();
  const menu = page.locator(".searchable-picker__popover--menu");
  await expect(menu).toBeVisible();
  expect((await menu.boundingBox())!.width).toBeLessThanOrEqual(280);
  await menu.getByPlaceholder("Search projects").fill("missing");
  await expect(
    menu.getByRole("option", { name: "Sage project with a long name" }),
  ).toHaveCount(0);
  expect((await menu.boundingBox())!.width).toBe(224);
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await page.emulateMedia({ forcedColors: "active" });
  await project.focus();
  const tab = frame.locator(".composer-location");
  await expect(tab).toHaveCSS("outline-style", "solid");
  await expect(tab).toHaveCSS("outline-width", "2px");
  await page.emulateMedia({ forcedColors: "none" });
  await page.setViewportSize({ width: 1280, height: 1096 });
  await branch.click();
  const branches = page.getByRole("listbox", { name: /^Switch branch in / });
  await expect(
    branches.getByRole("option", { name: "Create new branch…" }),
  ).toBeVisible();
  await expect(
    branches.getByRole("option", { name: /Rename|Delete/ }),
  ).toHaveCount(0);
  await expect(
    page.locator(".searchable-picker__popover--menu"),
  ).toHaveScreenshot("composer-branch-menu.png");
  await page.getByPlaceholder("Select a branch").fill("release");
  await expect(
    branches.getByRole("option", { name: "Switch to release/beta" }),
  ).toBeVisible();
  await expect(branches.getByRole("option", { name: /main/ })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(branch).toBeFocused();
});

test("the location strip is gone once the chat has a turn", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer").first();
  await expect(page.locator(".composer-location")).toHaveCount(0);
  const turns = await page.evaluate(async () => {
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const projection = acpProjectionStoreFor(
      useWorkbenchStore.getState().activeTabId,
    ).getState();
    const { page: current, scope } = projection;
    if (!current || !scope) throw new Error("Expected a loaded Chat");
    projection.applyAuthoritativePage(scope, { ...current, turns: [] });
    return current.turns;
  });
  await expect(
    page.getByRole("group", { name: "Chat location" }),
  ).toBeVisible();
  await page.evaluate(async (turns) => {
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const projection = acpProjectionStoreFor(
      useWorkbenchStore.getState().activeTabId,
    ).getState();
    projection.applyAuthoritativePage(projection.scope!, {
      ...projection.page!,
      turns,
    });
  }, turns);
  await expect(page.locator(".composer-location")).toHaveCount(0);
  const add = composer.getByRole("button", {
    name: "Composer options",
    exact: true,
  });
  await add.click();
  await expect(
    page
      .getByRole("menu", { name: "Composer options menu", exact: true })
      .getByRole("menuitem", { name: "Work in a project", exact: true }),
  ).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(add).toBeFocused();
});

test("a real project named Choose project retains the clear control", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const store = useWorkbenchStore.getState;
    const projectId = store().project!.id;
    await store().openNewChatDraft(projectId);
    useWorkbenchStore.setState({
      project: { ...store().project!, name: "Choose project" },
      projects: store().projects.map((project) =>
        project.id === projectId
          ? { ...project, name: "Choose project" }
          : project,
      ),
    });
  });
  const location = page.getByRole("group", {
    name: "Chat location",
    exact: true,
  });
  await expect(
    location.getByRole("button", {
      name: "Project: Choose project",
      exact: true,
    }),
  ).toBeVisible();
  await location
    .getByRole("button", { name: "Project: Choose project", exact: true })
    .hover();
  await location
    .getByRole("button", { name: "Remove project from this chat" })
    .click();
  await expect(
    location.getByRole("button", { name: "Choose project", exact: true }),
  ).toBeVisible();
});

test("new chat can choose no project and distinguishes duplicate project names", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    useWorkbenchStore.setState({
      project: { ...state.project!, name: "Sage" },
      projects: [
        {
          id: state.project!.id,
          name: "Sage",
          parentPath: "/work/first",
          rootCount: 1,
          lastOpenedAt: 1,
        },
        {
          id: "other-project",
          name: "Sage",
          parentPath: "/work/second",
          rootCount: 1,
          lastOpenedAt: 1,
        },
      ],
      unscopedChats: [
        {
          id: "private-chat-scope",
          name: "",
          rootCount: 1,
          lastOpenedAt: 1,
          railChats: [
            {
              id: "personal-chat",
              title: "Personal chat",
              pinned: false,
              position: 0,
              updatedAt: Date.now(),
            },
          ],
        },
      ],
    });
  });
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  await expect(
    sidebar
      .getByRole("region", { name: "Projects" })
      .getByText("Personal chat"),
  ).toHaveCount(0);
  await expect(
    sidebar
      .getByRole("region", { name: "Recents" })
      .getByRole("button", { name: "Open chat Personal chat", exact: true }),
  ).toBeVisible();
  await sidebar.getByRole("button", { name: "New chat", exact: true }).click();
  const location = page.getByRole("group", { name: "Chat location" });
  const choose = location.getByRole("button", {
    name: "Choose project",
    exact: true,
  });
  await expect(choose).toHaveAttribute(
    "title",
    "Change the project for this chat",
  );
  await expect(
    page.getByRole("region", { name: "New chat", exact: true }),
  ).not.toContainText("Sage");
  await choose.click();
  await expect(
    page.getByRole("option", { name: "Sage, /work/first", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("option", { name: "Sage, /work/second", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("combobox", { name: "Choose project", exact: true })
    .fill("missing project");
  await expect(page.getByRole("option", { name: /^Sage,/ })).toHaveCount(0);
  await expect(
    page.getByRole("option", { name: "New project", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("option", { name: "Don't work in a project", exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("combobox", { name: "Choose project", exact: true })
    .press("Escape");
  await choose.click();
  await page
    .getByRole("combobox", { name: "Choose project", exact: true })
    .fill("");
  await page
    .getByRole("option", { name: "Sage, /work/first", exact: true })
    .click();
  const selected = location.getByRole("button", {
    name: "Project: Sage",
    exact: true,
  });
  await expect(selected).toBeVisible();
  const draftComposer = page.getByRole("combobox", {
    name: "Message agent",
    exact: true,
  });
  await draftComposer.fill("Keep this project-selected unsent prompt");
  await sidebar.getByRole("button", { name: "New chat", exact: true }).click();
  await expect(draftComposer).toBeFocused();
  await expect(draftComposer).toHaveValue(
    "Keep this project-selected unsent prompt",
  );
  // The global New chat keeps unsent text but always opens project-less.
  await expect(choose).toBeVisible();
  await expect(selected).toHaveCount(0);
  await choose.click();
  await page
    .getByRole("option", { name: "Sage, /work/first", exact: true })
    .click();
  await expect(selected).toBeVisible();
  const clear = location.locator(".project-scope__clear");
  await selected.hover();
  await expect(clear).toBeVisible();
  await clear.click();
  await expect(choose).toBeVisible();
  await choose.click();
  await page
    .getByRole("option", { name: "Sage, /work/second", exact: true })
    .click();
  await page.mouse.move(0, 0);
  await page
    .getByRole("combobox", { name: "Message agent", exact: true })
    .focus();
  await expect(clear).toBeHidden();
  await location
    .getByRole("button", { name: "Project: Sage", exact: true })
    .focus();
  await expect(clear).toBeVisible();
  await clear.click();
  await expect(choose).toBeVisible();
});

test("project selection keeps the visible chat's identity with a right-pane File open", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  const expected = await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { resetBrowserDesktopForTests } =
      await import("/src/services/browserDesktop.ts");
    resetBrowserDesktopForTests();
    await useWorkbenchStore.getState().initialize();
    const projectId = useWorkbenchStore.getState().project!.id;
    await useWorkbenchStore.getState().createChat(null);
    const state = useWorkbenchStore.getState();
    const chatId = state.activeTabId!;
    await state.openFile({
      rootId: state.project!.roots[0].id,
      relativePath: "notes.txt",
    });
    return { projectId, chatId };
  });
  await expect(page.locator(".agent-rail-identity strong")).toHaveText("Local");
  await expect(page.locator(".account-avatar")).toHaveText("L");
  await page
    .getByRole("button", { name: "Composer options", exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Work in a project", exact: true })
    .click();
  await page.getByRole("option", { name: "Sage", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        const state = useWorkbenchStore.getState();
        return { projectId: state.project?.id, chatId: state.activeTabId };
      }),
    )
    .toEqual(expected);
  await expect(page.locator(".agent-rail-identity strong")).toHaveText("Sage");
});

test("New project preserves the unsent composer through creation and reparenting", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  const chat = await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { resetBrowserDesktopForTests } =
      await import("/src/services/browserDesktop.ts");
    resetBrowserDesktopForTests();
    await useWorkbenchStore.getState().initialize();
    await useWorkbenchStore.getState().createChat(null);
    // Native creation persists the chat; explicitly open its composer rather
    // than the unsaved draft shown while the initial project was empty.
    await useWorkbenchStore
      .getState()
      .activateTab(useWorkbenchStore.getState().activeTabId!);
    const state = useWorkbenchStore.getState();
    const chat = state.activeTabId!;
    const owner = state.captureChatOwnership(chat)!;
    state.setChatComposer(owner, "Keep my prompt and reference");
    state.setChatHarnessSelection(owner, {
      harnessId: "sage-simulator",
      modelId: "sim-fast",
    });
    if (
      !state.attachChatContext(owner, {
        kind: "browserPage",
        tabId: "browser-1",
        url: "https://example.test/",
        title: "Reference",
        text: "Keep this context",
      })
    )
      throw new Error("context refused");
    return chat;
  });
  await expect(
    page.getByRole("combobox", { name: "Message agent", exact: true }),
  ).toHaveValue("Keep my prompt and reference");
  await page
    .getByRole("button", { name: "Composer options", exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Work in a project", exact: true })
    .click();
  await page.getByRole("option", { name: "New project", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Create project" });
  await dialog.getByRole("textbox", { name: "Project name" }).fill("Local");
  await dialog
    .getByRole("button", { name: "Create project", exact: true })
    .click();
  await expect(dialog).toBeHidden();
  await expect(
    page.getByRole("combobox", { name: "Message agent", exact: true }),
  ).toHaveValue("Keep my prompt and reference");
  await expect
    .poll(() =>
      page.evaluate(async (chatId) => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        const state = useWorkbenchStore.getState();
        return {
          active: state.activeTabId,
          project: state.project?.name,
          composer: state.chatUiById[chatId],
        };
      }, chat),
    )
    .toMatchObject({
      active: chat,
      project: "Local",
      composer: {
        harnessId: "sage-simulator",
        modelId: "sim-fast",
        attachedContext: [
          {
            kind: "browserPage",
            title: "Reference",
            text: "Keep this context",
          },
        ],
      },
    });
});

for (const when of ["before creation", "during creation"]) {
  test(`New project preserves the private composer when files appear ${when}`, async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const original = await page.evaluate(async (when) => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const { resetBrowserDesktopForTests } =
        await import("/src/services/browserDesktop.ts");
      const { getWorkbenchPorts } =
        await import("/src/ports/workbenchRuntime.ts");
      const { browserDirectories } =
        await import("/src/services/browserWorkspaceFixture.ts");
      const { resourceKey } = await import("/src/domain/resourceIdentity.ts");
      resetBrowserDesktopForTests();
      await useWorkbenchStore.getState().initialize();
      await useWorkbenchStore.getState().createChat(null);
      await useWorkbenchStore
        .getState()
        .activateTab(useWorkbenchStore.getState().activeTabId!);
      const state = useWorkbenchStore.getState();
      const owner = state.captureChatOwnership()!;
      state.setChatComposer(owner, "Keep this unsent prompt");
      state.attachChatContext(owner, {
        kind: "browserPage",
        tabId: "browser-1",
        url: "https://example.test/",
        title: "Reference",
        text: "Keep this attachment",
      });
      const parent = { rootId: state.project!.roots[0].id, relativePath: "" };
      const write = () =>
        browserDirectories.get(resourceKey(parent))!.entries.push({
          name: "notes.txt",
          kind: "file",
          byteLen: 12,
          resource: { ...parent, relativePath: "notes.txt" },
        });
      if (when === "before creation") {
        await getWorkbenchPorts().project.createProjectFile({
          projectId: state.project!.id,
          projectGeneration: state.projectGeneration,
          expectedTransitionEpoch: state.nativeTransitionEpoch,
          parent,
          basename: "notes.txt",
        });
      } else {
        const move = getWorkbenchPorts().chat.setChatProject.bind(
          getWorkbenchPorts().chat,
        );
        getWorkbenchPorts().chat.setChatProject = async (...args) => {
          write();
          return move(...args);
        };
      }
      return {
        chat: state.activeTabId,
        project: state.project!.id,
        projects: state.projects.length,
      };
    }, when);
    await page
      .getByRole("button", { name: "Composer options", exact: true })
      .click();
    await page
      .getByRole("menuitem", { name: "Work in a project", exact: true })
      .click();
    await page
      .getByRole("option", { name: "New project", exact: true })
      .click();
    const dialog = page.getByRole("dialog", { name: "Create project" });
    await dialog
      .getByRole("textbox", { name: "Project name" })
      .fill("Destination");
    await dialog
      .getByRole("button", { name: "Create project", exact: true })
      .click();
    await expect(page.getByRole("alert")).toContainText(
      "This chat's folder has files. Start a new chat in the project instead.",
    );
    await expect(
      page.getByRole("combobox", { name: "Message agent", exact: true }),
    ).toHaveValue("Keep this unsent prompt");
    await expect
      .poll(() =>
        page.evaluate(async () => {
          const { useWorkbenchStore } = await import("/src/app/store.ts");
          const state = useWorkbenchStore.getState();
          return {
            chat: state.activeTabId,
            project: state.project?.id,
            projects: state.projects.length,
            context: state.chatUiById[state.activeTabId!]?.attachedContext,
          };
        }),
      )
      .toMatchObject({
        ...original,
        projects: original.projects + (when === "during creation" ? 1 : 0),
        context: [{ title: "Reference", text: "Keep this attachment" }],
      });
  });
}

test("active project-less chat can be archived and restored from Recents", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { resetBrowserDesktopForTests } =
      await import("/src/services/browserDesktop.ts");
    resetBrowserDesktopForTests();
    await useWorkbenchStore.getState().initialize();
    await useWorkbenchStore.getState().createChat(null);
    await useWorkbenchStore
      .getState()
      .activateTab(useWorkbenchStore.getState().activeTabId!);
    useWorkbenchStore.setState({
      tabs: useWorkbenchStore
        .getState()
        .tabs.map((tab) => ({ ...tab, updatedAt: Date.now() })),
    });
  });
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  await sidebar
    .getByRole("button", { name: "Open chat Chat", exact: true })
    .hover();
  await sidebar
    .getByRole("button", { name: "Archive Chat", exact: true })
    .click();
  await sidebar
    .getByRole("button", { name: "Archived chats: 1", exact: true })
    .click();
  await expect(
    sidebar.getByRole("button", { name: "Delete chat Chat", exact: true }),
  ).toBeVisible();
  await sidebar
    .getByRole("button", { name: "Restore chat Chat", exact: true })
    .click();
  await expect(
    sidebar.getByRole("button", { name: "Archived chats: 1", exact: true }),
  ).toHaveCount(0);
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        const state = useWorkbenchStore.getState();
        return {
          internal: state.project?.isInternal,
          chats: state.tabs.filter((tab) => tab.kind === "chat").length,
          archived: state.archivedChats.length,
        };
      }),
    )
    .toEqual({ internal: true, chats: 1, archived: 0 });
});

test("a side chat composer shows no location strip", async ({ page }) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openSplitComposerScenario(page);
  const composers = page.locator("form.composer");
  await expect(composers).toHaveCount(2);
  await expect(page.locator(".composer-location")).toHaveCount(0);
  for (const composer of await composers.all())
    await expectResponsiveToolbar(composer);
});

test("reserves clickable icon targets while allowing the harness to shrink", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer").first();
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
    fixture.installVisualComposerEffortFixture({
      levels: ["low", "medium", "high"],
      currentValue: "high",
    });
  });
  const model = composer.getByLabel("Model", { exact: true });
  await expect(model.locator(".composer__model-effort")).toHaveText("High");
  await expect(model).toHaveCSS("min-width", "28px");
  expect((await model.boundingBox())!.width).toBeGreaterThanOrEqual(28);
  await composer.evaluate((element) => {
    element.style.width = "320px";
  });
  await expectResponsiveToolbar(composer);
});
