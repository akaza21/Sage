import { expect, test, type Locator } from "@playwright/test";
import {
  expectComposerSweep,
  settledComposerFit,
  sweepComposerToolbar,
} from "./shared/composerGeometry";
import {
  waitForAnimations,
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
      const model = composer.getByLabel("Model", { exact: true });
      const label = model.locator(".composer__model-label");
      await expect(label).toHaveText(profile.model);

      // The toolbar moves Project and then effort into the options menu only
      // when the row cannot fit them even as icons.
      const fit = await settledComposerFit(composer);
      const compactProject = fit.includes("project-menu");
      const compactEffort = fit.includes("effort-menu");
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
            // Controls lead the row from the Add button (when offered); the
            // mic (when offered) and send trail it.
            order: (() => {
              const box = (selector: string) =>
                toolbar.querySelector(selector)?.getBoundingClientRect();
              const send = box(".send-button")!;
              const mic = box(".composer__mic");
              const plus = box(".composer__plus");
              return (
                boxes.every((other) => other.right <= send.right) &&
                (mic === undefined || mic.right <= send.left) &&
                (plus === undefined ||
                  boxes.every((other) => other.left >= plus.left))
              );
            })(),
          };
        });
      expect(geometry).toEqual({
        overflow: false,
        contained: true,
        aligned: true,
        disjoint: true,
        hitTestable: true,
        order: true,
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
        (Boolean(profile.effort) && compactEffort);
      const menu = page.getByRole("menu", { name: "Composer options menu" });
      if (hasAddActions) {
        await add.click();
        await expect(menu.getByText("Add", { exact: true })).toBeVisible();
        await expect(
          menu.getByRole("menuitem", { name: "Goal", exact: true }),
        ).toHaveCount(profile.goal ? 1 : 0);
        await expect(
          menu.getByRole("menuitem", { name: "Thinking level", exact: true }),
        ).toHaveCount(profile.effort && compactEffort ? 1 : 0);
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
      } else await expect(add).toBeDisabled();
      if (!profile.effort) {
        if (hasAddActions) await page.keyboard.press("Escape");
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
      await waitForAnimations(effort);
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
      composer.locator(".composer__effort-picker[data-overflow-control]"),
  },
  // A name shorter than the readable prefix must not need more room once the
  // model name starts to truncate.
  "with a short model name": {
    install: async () => {
      const fixture =
        await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
      fixture.installVisualComposerEffortFixture({
        levels: ["medium", "extra high"],
        currentValue: "extra high",
        modelName: "GPT-5",
      });
    },
    ready: (composer: Locator) =>
      composer.getByLabel("Model", { exact: true }).filter({
        hasText: "GPT-5",
      }),
  },
};

// Each state sweeps 640px to 200px in 2px steps, in ranges short enough to
// finish within the test timeout under CPU load. The widest composer has room
// for every label and the context meter.
const SWEEP_RANGES = [
  [640, 562],
  [560, 442],
  [440, 322],
  [320, 200],
] as const;

for (const [state, { install, ready }] of Object.entries(toolbarStates)) {
  for (const [from, to] of SWEEP_RANGES)
    test(`Composer toolbar compacts its labels in priority order ${state} from ${from}px to ${to}px`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1888, height: 1096 });
      await openScenario(page, "agent-composer-context-known", "agent");
      await settleChatProjections(page);
      await page.evaluate(install);
      const composer = page.locator("form.composer").first();
      await expect(ready(composer)).toHaveCount(1);
      await expect(
        composer.getByLabel("Composer context", { exact: true }),
      ).toBeVisible();
      await expect(
        composer.locator(".composer__effort-label").first(),
      ).toBeVisible();
      const widths = Array.from(
        { length: (from - to) / 2 + 1 },
        (_, index) => from - index * 2,
      );
      const sweep = await sweepComposerToolbar(composer, widths);

      // The widest composer shows every label whole.
      if (from === SWEEP_RANGES[0][0])
        expect(sweep[0].labels.filter((label) => label.present)).toEqual(
          sweep[0].labels
            .filter((label) => label.present)
            .map(() => ({ present: true, shown: true, clipped: false })),
        );
      expectComposerSweep(sweep);
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
