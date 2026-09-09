import { expect, test } from "@playwright/test";

import { AGENT_VISUAL_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { VISUAL_SCENARIOS } from "./fixtures/visualScenarioCatalog";
import {
  expectNoViewportOverflow,
  openScenario,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";
import {
  expandChildActivityHierarchy,
  expectAgentImageContained,
  expectChildActivityContained,
  runAgentCanonicalTest,
} from "./agentVisualHelpers";

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const { name, mode } of VISUAL_SCENARIOS.filter((scenario) =>
    AGENT_VISUAL_SCENARIOS.includes(scenario.name),
  )) {
    test(`${name} stays visually stable`, async ({ page }) => {
      await runAgentCanonicalTest(page, name, mode);
    });
  }
});

test.describe("V5b Agent image presentation", () => {
  test("stays contained at narrow width, forced colors, and reduced motion", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    await openScenario(page, "agent-image", "agent");

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expectAgentImageContained(page);
    await expectNoViewportOverflow(page);

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    expect(
      await page.locator(".acp-image").evaluate((figure) => {
        const figureStyle = getComputedStyle(figure);
        const imageStyle = getComputedStyle(
          figure.querySelector(".acp-image__src") ?? figure,
        );
        return {
          borderStyle: figureStyle.borderTopStyle,
          borderColor: figureStyle.borderTopColor,
          animationDuration: imageStyle.animationDuration,
          transitionDuration: imageStyle.transitionDuration,
        };
      }),
    ).toEqual({
      borderStyle: "solid",
      borderColor: "rgb(255, 255, 255)",
      animationDuration: "0s",
      transitionDuration: "0s",
    });
  });
});

test.describe("V5c Agent child activity presentation", () => {
  test("keeps four read-only levels usable at narrow width and forced colors", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    await openScenario(page, "agent-child-activity", "agent");
    const { activity, tree } = await expandChildActivityHierarchy(page);

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expectChildActivityContained(page);
    const longLabel =
      "Coordinate the independent accessibility review across every visible workbench surface";
    const collaboration = page.getByRole("group", {
      name: `Harness collaboration: ${longLabel}`,
    });
    await expect(collaboration).toBeVisible();
    await expect(
      collaboration.locator(".acp-collaboration__label"),
    ).toHaveAttribute("title", longLabel);
    expect(
      await collaboration
        .locator(".acp-collaboration__label")
        .evaluate((label) => label.scrollWidth > label.clientWidth),
    ).toBe(true);
    await expectNoViewportOverflow(page);

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    expect(
      await activity.evaluate((node) => {
        const item = node.querySelector<HTMLElement>(
          ".acp-child-activity__item",
        );
        const itemStyle = item ? getComputedStyle(item, "::before") : null;
        const label = node.querySelector<HTMLElement>(
          ".acp-collaboration__label",
        );
        return {
          activityBorderStyle: getComputedStyle(node).borderLeftStyle,
          connectorBorderStyle: itemStyle?.borderTopStyle,
          animationDuration: label
            ? getComputedStyle(label).animationDuration
            : null,
          transitionDuration: label
            ? getComputedStyle(label).transitionDuration
            : null,
        };
      }),
    ).toEqual({
      activityBorderStyle: "solid",
      connectorBorderStyle: "solid",
      animationDuration: "0s",
      transitionDuration: "0s",
    });
    await expect(tree.locator('[aria-level="4"]')).toBeFocused();
  });
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("keeps dual Chat header geometry stable when focus changes", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "agent-dual", "agent");

    const panes = page.locator(".agent-chat-pane");
    const paneHeaders = page.locator(".agent-chat-pane__header");
    const actionRows = page.locator(".agent-chat-pane .surface-header");
    await expect(panes).toHaveCount(2);
    await expect(paneHeaders).toHaveCount(2);
    await expect(actionRows).toHaveCount(2);
    const sidebarSplitter = page.locator(".workbench-sidebar-splitter");
    const chatSplitter = page.getByRole("separator", {
      name: "Resize chat panes",
    });
    await expect(sidebarSplitter).toHaveCount(1);
    await expect(chatSplitter).toHaveCount(1);
    expect(
      await page.evaluate(() => {
        const chatRow =
          document.querySelector<HTMLElement>(".project-chat-row");
        const chat = document.querySelector<HTMLElement>(".project-chat");
        const working = document.querySelector<HTMLElement>(
          ".project-chat__working",
        );
        const splitter = document.querySelector<HTMLElement>(
          ".agent-chat-splitter",
        );
        if (!chatRow || !chat || !working || !splitter) return null;
        return {
          chat: getComputedStyle(chat).borderRadius,
          chatRow: getComputedStyle(chatRow).borderRadius,
          splitter: getComputedStyle(splitter, "::after").borderRadius,
          working: getComputedStyle(working).borderRadius,
        };
      }),
    ).toEqual({
      chat: "6px",
      chatRow: "6px",
      splitter: "999px",
      working: "50%",
    });
    expect(
      await sidebarSplitter.evaluate(
        (node) => getComputedStyle(node, "::after").width,
      ),
    ).toBe("2px");
    expect(
      await chatSplitter.evaluate(
        (node) => getComputedStyle(node, "::after").width,
      ),
    ).toBe("2px");
    await chatSplitter.focus();
    await page.keyboard.press("Tab");
    await page.keyboard.press("Shift+Tab");
    await expect(chatSplitter).toBeFocused();
    expect(
      await chatSplitter.evaluate((node) => ({
        focusVisible: node.matches(":focus-visible"),
        lineWidth: getComputedStyle(node, "::after").width,
      })),
    ).toEqual({ focusVisible: true, lineWidth: "1px" });
    await chatSplitter.evaluate((node) => (node as HTMLElement).blur());
    expect(
      await paneHeaders.evaluateAll((headers) =>
        headers.map((header) => header.getBoundingClientRect().height),
      ),
    ).toEqual([32, 32]);
    expect(
      await actionRows.evaluateAll((rows) =>
        rows.every((row) => row.getBoundingClientRect().height <= 36),
      ),
    ).toBe(true);
    const shortcuts = page.locator(
      ".agent-chat-panes--split .composer__shortcut",
    );
    await expect(shortcuts).toHaveCount(2);
    await expect(shortcuts.first()).toBeHidden();
    for (let index = 0; index < 2; index += 1) {
      const pane = panes.nth(index);
      const controls = await Promise.all(
        [
          pane.getByRole("combobox", { name: "Harness" }),
          pane.getByRole("combobox", { name: "Model" }),
          pane.locator(".send-button"),
        ].map((control) =>
          control.evaluate((node) => node.getBoundingClientRect().y),
        ),
      );
      expect(Math.max(...controls) - Math.min(...controls)).toBeLessThan(4);
    }

    const before = await panes.evaluateAll((items) =>
      items.map((item) => {
        const bounds = item.getBoundingClientRect();
        return [bounds.x, bounds.y, bounds.width, bounds.height];
      }),
    );
    const secondFocus = panes.nth(1).locator(".agent-chat-pane__focus");
    await secondFocus.click();
    await expect(secondFocus).toHaveAttribute("aria-pressed", "true");
    const after = await panes.evaluateAll((items) =>
      items.map((item) => {
        const bounds = item.getBoundingClientRect();
        return [bounds.x, bounds.y, bounds.width, bounds.height];
      }),
    );
    expect(after).toEqual(before);
    await expectNoViewportOverflow(page);
  });

  test("shows only the focused Chat when the measured center becomes tight", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_000, height: 720 });
    const workbench = await openScenario(page, "agent-dual", "agent");

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("region", {
        name: "Chat pane Agent navigation polish",
      }),
    ).toBeVisible();

    await page
      .getByRole("complementary", { name: "Projects and chats" })
      .getByRole("button", { name: /Open chat Editor consistency pass/ })
      .click();
    await expect(
      page.getByRole("region", {
        name: "Chat pane Editor consistency pass",
      }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-dual-medium.png");
  });

  test("supports a 720px narrow Agent window", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(page, "agent-dual", "agent");

    await expect(
      page.getByRole("region", { name: "Agent chat", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-narrow-720x560.png");
  });

  test("exposes visible keyboard focus", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "agent-single", "agent");

    const composerSelector =
      'textarea[aria-label="Message agent"]:not(:disabled)';
    const composerTextarea = page.locator(composerSelector);
    const currentProjection = page.locator(
      ".agent-chat-pane--focused .acp-chat-projection",
    );
    await expect(currentProjection).toHaveAttribute(
      "data-project-id",
      "visual-project",
    );
    await expect(currentProjection).toHaveAttribute(
      "data-chat-id",
      "visual-chat-agent-single",
    );
    await expect(currentProjection).toHaveAttribute(
      "data-current-session-id",
      "visual-session",
    );
    await expect(composerTextarea).toBeVisible();
    await expect(composerTextarea).toBeEnabled();
    await page.keyboard.press("ControlOrMeta+L");
    await expect(composerTextarea).toBeFocused();
    expect(
      await composerTextarea.evaluate(
        (textarea) => document.activeElement === textarea,
      ),
    ).toBe(true);
    expect(
      await composerTextarea.evaluate((textarea) =>
        textarea.matches(":focus-visible"),
      ),
    ).toBe(true);
    const focusVisible = page.locator(":focus-visible");
    await expect(focusVisible).toHaveCount(1);
    expect(
      await focusVisible.evaluate(
        (focused, selector) => focused === document.querySelector(selector),
        composerSelector,
      ),
    ).toBe(true);
    await expect
      .poll(
        () =>
          focusVisible.evaluate((trigger) => {
            const owner = trigger.closest<HTMLElement>(".composer");
            if (!owner) return false;
            const edgeProbe = document.createElement("span");
            edgeProbe.style.border = "1px solid var(--highlight-focus-edge)";
            owner.append(edgeProbe);
            const target = getComputedStyle(edgeProbe).borderTopColor;
            edgeProbe.remove();
            return getComputedStyle(owner).borderTopColor === target;
          }),
        { timeout: 1_000 },
      )
      .toBe(true);
    const sharedFocus = await composerTextarea.evaluate((trigger) => {
      const identity = (element: Element | null) => {
        if (!element) return "none";
        const id = element.id ? `#${element.id}` : "";
        const classes = [...element.classList]
          .map((className) => `.${className}`)
          .join("");
        return `${element.tagName.toLowerCase()}${id}${classes}`;
      };
      const transparentProbe = document.createElement("span");
      transparentProbe.style.border = "1px solid transparent";
      document.body.append(transparentProbe);
      const transparentColor =
        getComputedStyle(transparentProbe).borderTopColor;
      transparentProbe.remove();

      const owner = trigger.closest<HTMLElement>(".composer");
      const ownerStyle = owner ? getComputedStyle(owner) : null;
      const workbench = trigger.closest<HTMLElement>(".workbench");
      const workbenchStyle = workbench ? getComputedStyle(workbench) : null;
      const halo = ownerStyle?.boxShadow ?? "none";
      const sharedStyleProbe = document.createElement("span");
      sharedStyleProbe.style.border = "1px solid var(--highlight-focus-edge)";
      sharedStyleProbe.style.boxShadow =
        "var(--highlight-focus-halo-shadow), var(--elevation-popover)";
      (owner ?? document.body).append(sharedStyleProbe);
      const sharedProbeStyle = getComputedStyle(sharedStyleProbe);
      const resolvedEdge = sharedProbeStyle.borderTopColor;
      const resolvedHalo = sharedProbeStyle.boxShadow;
      sharedStyleProbe.remove();
      return {
        edgeToken: workbenchStyle
          ?.getPropertyValue("--highlight-focus-edge")
          .trim(),
        haloToken: workbenchStyle
          ?.getPropertyValue("--highlight-focus-halo")
          .trim(),
        edgeColor: ownerStyle?.borderTopColor,
        edgeStyle: ownerStyle?.borderTopStyle,
        edgeWidth: ownerStyle?.borderTopWidth,
        focusRingOffset: ownerStyle
          ?.getPropertyValue("--focus-ring-offset")
          .trim(),
        focusWithinMatches: owner?.matches(":focus-within") ?? false,
        halo,
        ownerContainsTrigger: owner?.contains(trigger) ?? false,
        ownerIdentity: identity(owner),
        resolvedEdge,
        resolvedHalo,
        transparentColor,
        triggerIdentity: identity(trigger),
      };
    });
    const focusIdentity = `trigger=${sharedFocus.triggerIdentity}; owner=${sharedFocus.ownerIdentity}`;
    expect(sharedFocus, focusIdentity).toMatchObject({
      edgeStyle: "solid",
      edgeWidth: "1px",
      focusRingOffset: "1px",
      focusWithinMatches: true,
      ownerContainsTrigger: true,
    });
    expect(sharedFocus.edgeToken, focusIdentity).toContain("color-mix");
    expect(sharedFocus.edgeToken, focusIdentity).toContain("transparent");
    expect(sharedFocus.haloToken, focusIdentity).toContain("color-mix");
    expect(sharedFocus.haloToken, focusIdentity).toContain("transparent");
    expect(sharedFocus.edgeColor, focusIdentity).not.toBe(
      sharedFocus.transparentColor,
    );
    expect(sharedFocus.edgeColor, focusIdentity).toBe(sharedFocus.resolvedEdge);
    expect(sharedFocus.halo, focusIdentity).not.toBe("none");
    expect(sharedFocus.halo, focusIdentity).toBe(sharedFocus.resolvedHalo);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-keyboard-focus.png");
  });

  test("honors forced colors", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ forcedColors: "active" });
    const workbench = await openScenario(page, "agent-single", "agent");

    const focusedPaneHeader = page.locator(
      ".agent-chat-pane--focused .agent-chat-pane__header",
    );
    const forcedFocus = await focusedPaneHeader.evaluate((node) => {
      const transparentProbe = document.createElement("span");
      transparentProbe.style.color = "transparent";
      document.body.append(transparentProbe);
      const transparentColor = getComputedStyle(transparentProbe).color;
      transparentProbe.remove();
      const style = getComputedStyle(node);
      return {
        outlineColor: style.outlineColor,
        outlineStyle: style.outlineStyle,
        outlineWidth: style.outlineWidth,
        shadow: style.boxShadow,
        transparentColor,
      };
    });
    expect(forcedFocus).toMatchObject({
      outlineStyle: "solid",
      outlineWidth: "2px",
      shadow: "none",
    });
    expect(forcedFocus.outlineColor).not.toBe(forcedFocus.transparentColor);

    const composerSelector =
      'textarea[aria-label="Message agent"]:not(:disabled)';
    const composerTextarea = page.locator(composerSelector);
    await page.keyboard.press("Tab");
    await expect(composerTextarea).toBeEnabled();
    await expect(composerTextarea).toBeFocused();
    await expect(composerTextarea).toBeVisible();
    expect(
      await composerTextarea.evaluate((textarea) =>
        textarea.matches(":focus-visible"),
      ),
    ).toBe(true);
    const keyboardFocus = page.locator(":focus-visible");
    await expect(keyboardFocus).toHaveCount(1);
    expect(
      await keyboardFocus.evaluate(
        (focused, selector) => focused === document.querySelector(selector),
        composerSelector,
      ),
    ).toBe(true);
    await expect
      .poll(
        () =>
          keyboardFocus.evaluate((trigger) => {
            const owner = trigger.closest<HTMLElement>(".composer");
            if (!owner) return false;
            const borderProbe = document.createElement("span");
            borderProbe.style.border = "1px solid Highlight";
            owner.append(borderProbe);
            const target = getComputedStyle(borderProbe).borderTopColor;
            borderProbe.remove();
            return getComputedStyle(owner).borderTopColor === target;
          }),
        { timeout: 1_000 },
      )
      .toBe(true);
    const forcedKeyboardFocus = await composerTextarea.evaluate((trigger) => {
      const identity = (element: Element | null) => {
        if (!element) return "none";
        const id = element.id ? `#${element.id}` : "";
        const classes = [...element.classList]
          .map((className) => `.${className}`)
          .join("");
        return `${element.tagName.toLowerCase()}${id}${classes}`;
      };
      const owner = trigger.closest<HTMLElement>(".composer");
      const ownerStyle = owner ? getComputedStyle(owner) : null;
      const systemProbe = document.createElement("span");
      systemProbe.style.border = "1px solid Highlight";
      systemProbe.style.outline = "2px solid Highlight";
      (owner ?? document.body).append(systemProbe);
      const systemProbeStyle = getComputedStyle(systemProbe);
      const resolvedBorderHighlight = systemProbeStyle.borderTopColor;
      const resolvedOutlineHighlight = systemProbeStyle.outlineColor;
      systemProbe.style.borderColor = "transparent";
      systemProbe.style.outlineColor = "transparent";
      const transparentProbeStyle = getComputedStyle(systemProbe);
      const transparentBorder = transparentProbeStyle.borderTopColor;
      const transparentOutline = transparentProbeStyle.outlineColor;
      systemProbe.remove();
      return {
        borderColor: ownerStyle?.borderTopColor,
        borderStyle: ownerStyle?.borderTopStyle,
        borderWidth: ownerStyle?.borderTopWidth,
        focusWithinMatches: owner?.matches(":focus-within") ?? false,
        outlineColor: ownerStyle?.outlineColor,
        outlineOffset: ownerStyle?.outlineOffset,
        outlineStyle: ownerStyle?.outlineStyle,
        outlineWidth: ownerStyle?.outlineWidth,
        ownerContainsTrigger: owner?.contains(trigger) ?? false,
        ownerIdentity: identity(owner),
        resolvedBorderHighlight,
        resolvedOutlineHighlight,
        shadow: ownerStyle?.boxShadow,
        transparentBorder,
        transparentOutline,
        triggerIdentity: identity(trigger),
      };
    });
    const forcedIdentity = `trigger=${forcedKeyboardFocus.triggerIdentity}; owner=${forcedKeyboardFocus.ownerIdentity}`;
    expect(forcedKeyboardFocus, forcedIdentity).toMatchObject({
      borderStyle: "solid",
      borderWidth: "1px",
      focusWithinMatches: true,
      outlineOffset: "-2px",
      outlineStyle: "solid",
      outlineWidth: "2px",
      ownerContainsTrigger: true,
      shadow: "none",
    });
    expect(forcedKeyboardFocus.borderColor, forcedIdentity).toBe(
      forcedKeyboardFocus.resolvedBorderHighlight,
    );
    expect(forcedKeyboardFocus.outlineColor, forcedIdentity).toBe(
      forcedKeyboardFocus.resolvedOutlineHighlight,
    );
    expect(forcedKeyboardFocus.borderColor, forcedIdentity).not.toBe(
      forcedKeyboardFocus.transparentBorder,
    );
    expect(forcedKeyboardFocus.outlineColor, forcedIdentity).not.toBe(
      forcedKeyboardFocus.transparentOutline,
    );
    await keyboardFocus.evaluate((trigger) => (trigger as HTMLElement).blur());
    await expect(page.locator(":focus-visible")).toHaveCount(0);

    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-forced-colors.png");
  });
});

test.describe("Wave 2 200% zoom state", () => {
  test.use({
    viewport: { width: 690, height: 440 },
    deviceScaleFactor: 2,
  });

  test("supports the Agent workbench at 200% zoom", async ({ page }) => {
    const workbench = await openScenario(page, "agent-dual", "agent");

    await expect(workbench).toHaveClass(/workbench--compact/);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expect(
      page.getByRole("region", { name: "Agent chat", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-zoom-200.png");
  });

  test("keeps Agent activity cards bounded at 200% zoom", async ({ page }) => {
    await openScenario(page, "agent-activity-family", "agent");

    await expect
      .poll(() =>
        page
          .locator(".acp-activity-card")
          .evaluateAll((cards) =>
            cards.every(
              (card) =>
                card.clientWidth > 0 && card.scrollWidth <= card.clientWidth,
            ),
          ),
      )
      .toBe(true);
    await expectNoViewportOverflow(page);
  });

  test("keeps an Agent image contained at 200% zoom", async ({ page }) => {
    await openScenario(page, "agent-image", "agent");

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expectAgentImageContained(page);
    await expectNoViewportOverflow(page);
  });

  test("keeps four child-activity levels contained at 200% zoom", async ({
    page,
  }) => {
    await openScenario(page, "agent-child-activity", "agent");
    await expandChildActivityHierarchy(page);

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expectChildActivityContained(page);
    await expectNoViewportOverflow(page);
  });
});
