import { expect, test, type Locator } from "@playwright/test";
import { settledComposerFit } from "./shared/composerGeometry";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

const profiles = [
  {
    name: "Codex",
    model: "GPT-6 Astra",
    effort: "effort",
    goal: true,
    modes: ["Default", "Plan"],
    access: true,
  },
  {
    name: "Claude Code",
    model: "Claude Opus",
    effort: "effort",
    goal: false,
    modes: ["Default", "Plan"],
    access: true,
  },
  {
    name: "Pi",
    model: "Pi confirmed model",
    effort: "thought_level",
    goal: false,
    modes: [],
    access: true,
  },
  {
    name: "OpenCode",
    model: "OpenCode confirmed model",
    effort: null,
    goal: false,
    modes: ["Build", "Plan"],
    access: false,
  },
  {
    name: "Oh My Pi",
    model: "OMP confirmed model",
    effort: "thought_level",
    goal: false,
    modes: [],
    access: false,
  },
];

for (const width of [1888, 720, 480]) {
  for (const profile of profiles) {
    test(`${profile.name} capability fixture compacts before wrapping at ${width}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, "agent-composer-context-known", "agent");
      await settleChatProjections(page);
      await page.evaluate(async (profile) => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        const { acpProjectionStoreFor } =
          await import("/src/app/acpProjectionRegistry.ts");
        const state = useWorkbenchStore.getState();
        const chat = state.tabs.find((tab) => tab.kind === "chat")!;
        const harnessId =
          state.chatUiById[chat.id]?.harnessId ?? state.selectedHarnessId;
        useWorkbenchStore.setState({
          harnesses: state.harnesses.map((harness) =>
            harness.id === harnessId
              ? {
                  ...harness,
                  name: profile.name,
                  accessModes: profile.access
                    ? [
                        {
                          id: "readOnly",
                          label: "Read-only",
                          description: "Read files",
                          fallback: true,
                        },
                      ]
                    : undefined,
                }
              : harness,
          ),
        });
        const projection = acpProjectionStoreFor(chat.id).getState();
        projection.applyAuthoritativePage(projection.scope!, {
          ...projection.page!,
          sessions: projection.page!.sessions.map((session) => ({
            ...session,
            capabilities: {
              ...session.capabilities,
              protocolVersion: profile.goal
                ? "codex-app-server-observed-0.146.0"
                : "1.0",
            },
          })),
          sessionConfig: {
            sageAgentSessionId: projection.page!.currentSessionId!,
            runtimeEpoch: projection.page!.sessions[0].runtimeEpoch!,
            configRevision: "1",
            confirmedModel: { id: "visual-model", name: profile.model },
            modes: profile.modes.length
              ? {
                  currentModeId: profile.modes[0].toLowerCase(),
                  availableModes: profile.modes.map((name) => ({
                    id: name.toLowerCase(),
                    name,
                  })),
                }
              : null,
            configOptions: profile.effort
              ? [
                  {
                    id: profile.effort,
                    category: profile.effort,
                    name: "Thinking level",
                    type: "select",
                    currentValue: "medium",
                    options: [
                      { value: "medium", name: "Medium" },
                      { value: "high", name: "High" },
                    ],
                  },
                ]
              : [],
          },
          goal: profile.goal
            ? {
                kind: "goal",
                snapshot: {
                  objective: "Ship safely",
                  status: "active",
                  tokenBudget: null,
                  tokensUsed: 0,
                  timeUsedSeconds: 1,
                  createdAt: null,
                  updatedAt: null,
                },
              }
            : { kind: "unsupported" },
        });
      }, profile);
      const composer = page.locator("form.composer").first();
      const model = composer.getByRole("button", {
        name: "Model",
        exact: true,
      });
      // The toolbar moves Project and then effort into the options menu only
      // when the row cannot fit them even as icons.
      await composer.evaluate(
        () =>
          new Promise((resolve) =>
            requestAnimationFrame(() =>
              requestAnimationFrame(() => requestAnimationFrame(resolve)),
            ),
          ),
      );
      const compactProject =
        (await composer.getByRole("button", { name: /^Project:/ }).count()) ===
        0;
      const compactEffort =
        Boolean(profile.effort) &&
        (await composer.locator(".composer__effort-picker").isHidden());
      const label = model.locator(".composer__model-label");
      await expect(label).toHaveText(profile.model);
      await expect(model).not.toHaveText(/Medium/);
      await expect(
        composer.locator(".chat-goal-entry, .chat-goal-strip"),
      ).toHaveCount(profile.goal ? 1 : 0);
      const geometry = await composer
        .locator(".composer__toolbar")
        .evaluate((toolbar) => {
          const row = toolbar.getBoundingClientRect();
          const controls = [
            ...toolbar.querySelectorAll("button, [role=progressbar]"),
          ].filter((element) => element.getBoundingClientRect().width > 0);
          const boxes = controls.map((element) =>
            element.getBoundingClientRect(),
          );
          return {
            overflow: toolbar.scrollWidth > toolbar.clientWidth,
            contained: boxes.every(
              (box) =>
                box.left >= row.left &&
                box.right <= row.right &&
                box.top >= row.top &&
                box.bottom <= row.bottom,
            ),
            aligned: boxes.every(
              (box) =>
                Math.abs(
                  box.top + box.height / 2 - boxes[0].top - boxes[0].height / 2,
                ) <= 1,
            ),
            disjoint: boxes.every((box, index) =>
              boxes
                .slice(index + 1)
                .every(
                  (other) => box.right <= other.left || other.right <= box.left,
                ),
            ),
            hitTestable: controls.every((element, index) =>
              element.contains(
                document.elementFromPoint(
                  boxes[index].x + boxes[index].width / 2,
                  boxes[index].y + boxes[index].height / 2,
                ),
              ),
            ),
          };
        });
      expect(geometry).toEqual({
        overflow: false,
        contained: true,
        aligned: true,
        disjoint: true,
        hitTestable: true,
      });
      if (width === 480) {
        expect((await composer.boundingBox())!.width).toBeGreaterThanOrEqual(
          390,
        );
        expect((await composer.boundingBox())!.width).toBeLessThanOrEqual(400);
      }
      await composer
        .getByRole("button", { name: "Harness", exact: true })
        .click();
      await expect(
        page.locator(
          '.composer__harness-menu [aria-selected="true"] .searchable-picker__subtitle',
        ),
      ).toHaveText(`${profile.model}${profile.effort ? " Medium" : ""}`);
      await page.keyboard.press("Escape");
      const add = composer.getByRole("button", {
        name: "Composer options",
        exact: true,
      });
      const hasAddActions =
        profile.goal ||
        profile.modes.length > 0 ||
        compactProject ||
        (Boolean(profile.effort) && profile.access && compactEffort);
      const menu = page.getByRole("menu", { name: "Composer options menu" });
      if (hasAddActions) {
        await add.click();
        await expect(menu.getByText("Add", { exact: true })).toBeVisible();
        await expect(
          menu.getByRole("menuitem", { name: "Goal", exact: true }),
        ).toHaveCount(profile.goal ? 1 : 0);
        await expect(
          menu.getByRole("menuitem", { name: "Thinking level", exact: true }),
        ).toHaveCount(
          profile.effort && profile.access && compactEffort ? 1 : 0,
        );
        await expect(
          menu.getByRole("menuitem", { name: /^Project:/ }),
        ).toHaveCount(compactProject ? 1 : 0);
        if (profile.modes.includes("Plan")) {
          const plan = menu.getByRole("menuitemradio", {
            name: "Plan mode",
            exact: true,
          });
          await expect(plan).toHaveAccessibleDescription("Plan mode is off");
          await expect(
            plan.locator('svg[aria-hidden="true"]').first(),
          ).toBeVisible();
        }
      } else await expect(add).toHaveCount(0);
      if (!profile.effort) {
        if (hasAddActions) await page.keyboard.press("Escape");
        return;
      }
      if (!profile.access) {
        // No advertised access mode blocks the composer (CP-29d), so the
        // effort control is shown but cannot open a panel.
        if (hasAddActions) await page.keyboard.press("Escape");
        if (!compactEffort)
          await expect(
            composer.locator(".composer__effort-picker"),
          ).toBeDisabled();
        return;
      }
      const effortAnchor = compactEffort
        ? add
        : composer.locator(".composer__effort-picker");
      if (compactEffort) {
        await menu
          .getByRole("menuitem", { name: "Thinking level", exact: true })
          .click();
      } else {
        if (hasAddActions) await page.keyboard.press("Escape");
        await effortAnchor.click();
      }
      const effort = page.getByRole("dialog", { name: "Thinking level" });
      await expect(effort).toBeVisible();
      await expect(page.getByRole("slider")).toHaveAttribute(
        "aria-valuetext",
        "Medium",
      );
      await effort.evaluate((element) =>
        Promise.all(
          element.getAnimations().map((animation) => animation.finished),
        ),
      );
      const anchor = (await effortAnchor.boundingBox())!;
      const panel = (await effort.boundingBox())!;
      expect(panel.x).toBeGreaterThanOrEqual(8);
      expect(panel.x + panel.width).toBeLessThanOrEqual(width - 8);
      expect(
        Math.min(
          Math.abs(panel.y + panel.height - anchor.y),
          Math.abs(panel.y - anchor.y - anchor.height),
        ),
      ).toBeLessThanOrEqual(5);
      await page.keyboard.press("Escape");
      await expect(effortAnchor).toBeFocused();
    });
  }
}

/**
 * Toolbar labels from lowest to highest priority. A label may compact only
 * after every lower-priority label that is present has compacted.
 */
const LABEL_PRIORITY = [
  ".composer__effort-picker:not([aria-controls]) .composer__effort-label",
  ".composer__capacity",
  ".composer__access-text",
  ".composer__harness .searchable-picker__trigger-label",
  ".project-scope .searchable-picker__trigger-label",
  ".composer__effort-picker[aria-controls] .composer__effort-label",
  ".composer__model-label",
];

const toolbarStates = {
  "awaiting a session": {
    install: async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const { acpProjectionStoreFor } =
        await import("/src/app/acpProjectionRegistry.ts");
      const chat = useWorkbenchStore
        .getState()
        .tabs.find((tab) => tab.kind === "chat")!;
      const projection = acpProjectionStoreFor(chat.id).getState();
      projection.applyAuthoritativePage(projection.scope!, {
        ...projection.page!,
        sessions: projection.page!.sessions.map((session) => ({
          ...session,
          state: "closed",
        })),
      });
    },
    // Until a session confirms effort, the toolbar shows its placeholder.
    ready: (composer: Locator) =>
      composer.locator(".composer__effort-picker:not([aria-controls])"),
  },
  "with a confirmed model and effort": {
    install: async () => {
      const fixture =
        await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
      fixture.installVisualComposerEffortFixture({
        levels: ["medium", "extra high"],
        currentValue: "extra high",
      });
    },
    ready: (composer: Locator) =>
      composer.locator(".composer__effort-picker[aria-controls]"),
  },
};

for (const [state, { install, ready }] of Object.entries(toolbarStates)) {
  test(`Composer toolbar compacts its labels in priority order ${state}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(install);
    const composer = page.locator("form.composer").first();
    await expect(ready(composer)).toHaveCount(1);
    await expect(
      composer.locator(".composer__effort-label").first(),
    ).toBeVisible();
    const sweep = await composer.evaluate(async (element, priority) => {
      const frame = () =>
        new Promise((resolve) => requestAnimationFrame(resolve));
      const controls = element.querySelector<HTMLElement>(
        ".composer__controls",
      )!;
      const readableCharacter = (label: HTMLElement) => {
        const probe = document.createElement("span");
        probe.textContent = "0";
        probe.style.font = getComputedStyle(label).font;
        document.body.append(probe);
        const width = probe.getBoundingClientRect().width;
        probe.remove();
        return width;
      };
      const results = [];
      for (let width = 560; width >= 200; width -= 2) {
        element.style.width = `${width}px`;
        // A resize refits the row on a later frame; wait until it settles.
        let layout = "";
        for (let settled = 0; settled < 3;) {
          await frame();
          const next = [...controls.querySelectorAll("*")]
            .map((node) => node.getBoundingClientRect().right)
            .join();
          settled = next === layout ? settled + 1 : 0;
          layout = next;
        }
        const labels = priority.map((selector) => {
          const label = element.querySelector<HTMLElement>(selector);
          const box = label?.getBoundingClientRect();
          const shown = Boolean(box && box.width > 0);
          const host = label?.closest("button, .composer__capacity, label");
          const hostBox = host?.getBoundingClientRect();
          return {
            present: Boolean(label),
            shown,
            // A label may shorten only to an ellipsis after a readable
            // prefix of eight characters; it is never cut off by its control.
            clipped:
              shown &&
              (box!.right > hostBox!.right + 0.5 ||
                box!.left < hostBox!.left - 0.5 ||
                (label!.scrollWidth > label!.clientWidth &&
                  (getComputedStyle(label!).textOverflow !== "ellipsis" ||
                    label!.clientWidth < 8 * readableCharacter(label!) - 0.5))),
          };
        });
        const edge =
          controls.getBoundingClientRect().right -
          Number.parseFloat(getComputedStyle(controls).paddingRight);
        const overflow = [...controls.children].some(
          (child) =>
            !["absolute", "fixed"].includes(getComputedStyle(child).position) &&
            child.getBoundingClientRect().right > edge + 0.5,
        );
        results.push({ width, labels, overflow });
      }
      element.style.width = "";
      return results;
    }, LABEL_PRIORITY);

    expect(sweep[0].labels.filter((label) => label.present)).toEqual(
      sweep[0].labels
        .filter((label) => label.present)
        .map(() => ({ present: true, shown: true, clipped: false })),
    );
    let previous = sweep[0];
    for (const step of sweep) {
      const at = `at ${step.width}px`;
      expect(step.overflow, `controls overflow ${at}`).toBe(false);
      step.labels.forEach((label, index) => {
        expect(label.clipped, `${LABEL_PRIORITY[index]} clipped ${at}`).toBe(
          false,
        );
        if (!label.shown) return;
        for (const higher of step.labels.slice(index + 1))
          expect(
            !higher.present || higher.shown,
            `${LABEL_PRIORITY[index]} outlasts a higher-priority label ${at}`,
          ).toBe(true);
        expect(
          previous.labels[index].shown || !previous.labels[index].present,
          `${LABEL_PRIORITY[index]} reappears while narrowing ${at}`,
        ).toBe(true);
      });
      previous = step;
    }
  });
}

test("Composer toolbar refits when a control grows without a content change", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer").first();
  await composer.evaluate((element) => {
    (element as HTMLElement).style.width = "480px";
  });
  const before = await settledComposerFit(composer);
  // A style change is an attribute write: no text or child changes.
  await composer
    .locator(".composer__model-trigger")
    .evaluate((trigger: HTMLElement) => {
      trigger.style.marginInlineStart = "160px";
    });
  const after = await settledComposerFit(composer);
  expect(after.length).toBeGreaterThan(before.length);
  expect(
    await composer
      .locator(".composer__controls")
      .evaluate((row) => row.scrollWidth <= row.clientWidth),
  ).toBe(true);
});
