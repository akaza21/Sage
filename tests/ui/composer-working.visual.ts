import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

async function openComposer(page: Page, working = false) {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openScenario(
    page,
    working ? "agent-thinking-live" : "agent-single",
    "agent",
  );
  await settleChatProjections(page);
  return page.locator(".chat-dock");
}

async function border(dock: Locator) {
  return dock.evaluate((element) => {
    const style = getComputedStyle(element, "::after");
    return {
      name: style.animationName,
      state: style.animationPlayState,
      color: style.borderColor,
    };
  });
}

for (const theme of ["dark", "light"] as const) {
  for (const reducedMotion of ["no-preference", "reduce"] as const) {
    test(`composer idle, typing and working (${theme}, ${reducedMotion})`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: theme, reducedMotion });
      const dock = await openComposer(page);
      expect((await border(dock)).name).toBe("none");
      await expect(
        dock.getByRole("button", { name: /dictation|Voice input/ }),
      ).toBeVisible();
      await expect(dock).toHaveScreenshot(
        `composer-idle-${theme}-${reducedMotion}.png`,
      );
      await dock
        .getByRole("combobox", { name: "Message agent" })
        .fill("Review the composer working state");
      await expect(
        dock.getByRole("button", { name: "Send message", exact: true }),
      ).toBeVisible();
      await expect(dock).toHaveScreenshot(
        `composer-typing-${theme}-${reducedMotion}.png`,
      );

      await openScenario(page, "agent-thinking-live", "agent");
      await settleChatProjections(page);
      await page.evaluate(async () => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        const { acpProjectionStoreFor } =
          await import("/src/app/acpProjectionRegistry.ts");
        const state = useWorkbenchStore.getState();
        const id = state.activeTabId!;
        const projection = acpProjectionStoreFor(id).getState();
        projection.applyAuthoritativePage(projection.scope!, {
          ...projection.page!,
          sessions: projection.page!.sessions.map((session) => ({
            ...session,
            capabilities: { ...session.capabilities!, activeTurnSteer: true },
          })),
        });
        useWorkbenchStore.setState({
          chatUiById: {
            ...state.chatUiById,
            [id]: {
              ...state.chatUiById[id],
              queuedPrompts: [
                {
                  id: "working-follow-up",
                  text: "Keep the queue actions reachable",
                  attachedContext: [],
                  bindingId: "fixture",
                  modelId: null,
                  status: "queued",
                },
              ],
            },
          },
        });
      });
      await expect(
        dock.getByRole("button", { name: "Cancel turn" }),
      ).toBeVisible();
      await expect(
        dock.getByRole("button", {
          name: "Steer queued message: Keep the queue actions reachable",
        }),
      ).toBeVisible();
      const state = await border(dock);
      expect(state.name).toBe(
        reducedMotion === "reduce" ? "none" : "composer-working",
      );
      expect(state.color).toBe(
        await dock.evaluate((element) => {
          const probe = document.createElement("span");
          probe.style.color = "var(--color-accent)";
          element.append(probe);
          const color = getComputedStyle(probe).color;
          probe.remove();
          return color;
        }),
      );
      await expect(dock).toHaveScreenshot(
        `composer-working-${theme}-${reducedMotion}.png`,
      );
    });
  }
}

for (const endWhilePressed of [false, true]) {
  test(`Stop dictation keeps the spoken draft unsent${endWhilePressed ? " when recognition ends during the press" : ""}`, async ({
    page,
  }) => {
    await page.addInitScript(() => {
      type Result = (event: unknown) => void;
      class FakeRecognition {
        continuous = false;
        interimResults = false;
        lang = "";
        onresult: Result | null = null;
        onerror: (() => void) | null = null;
        onend: (() => void) | null = null;
        start() {
          Object.assign(window, { fakeRecognition: this });
        }
        stop() {}
        abort() {}
      }
      Object.assign(window, { SpeechRecognition: FakeRecognition });
      let submits = 0;
      document.addEventListener("submit", () => (submits += 1), true);
      Object.defineProperty(window, "submits", { get: () => submits });
    });
    const dock = await openComposer(page);
    const prompt = dock.getByRole("combobox", { name: "Message agent" });
    await dock.getByRole("button", { name: "Start dictation" }).click();
    const stop = dock.getByRole("button", { name: "Stop dictation" });
    await expect(stop).toHaveAttribute("aria-pressed", "true");
    await page.evaluate(() => {
      const result = Object.assign([{ transcript: "Review the composer" }], {
        isFinal: true,
      });
      const { fakeRecognition } = window as unknown as {
        fakeRecognition: { onresult: (event: unknown) => void };
      };
      fakeRecognition.onresult({ resultIndex: 0, results: [result] });
    });
    await expect(prompt).toHaveValue("Review the composer");
    if (endWhilePressed) {
      // Press the button padding: its icon is replaced when recognition ends.
      const box = await stop.boundingBox();
      expect(box).not.toBeNull();
      await stop.hover({ position: { x: 4, y: box!.height / 2 } });
      await page.mouse.down();
      await page.evaluate(() => {
        const { fakeRecognition } = window as unknown as {
          fakeRecognition: { onend: () => void };
        };
        fakeRecognition.onend();
      });
      await expect(
        dock.getByRole("button", { name: "Send message", exact: true }),
      ).toBeVisible();
      await page.mouse.up();
    } else {
      await stop.click();
    }
    await expect(
      dock.getByRole("button", { name: "Send message", exact: true }),
    ).toBeVisible();
    await expect(prompt).toHaveValue("Review the composer");
    expect(
      await page.evaluate(
        () => (window as unknown as { submits: number }).submits,
      ),
    ).toBe(0);
  });
}

test("working border sleeps with the document and wakes on return", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  const dock = await openComposer(page, true);
  const running = () =>
    page.evaluate(
      () =>
        document
          .getAnimations()
          .filter(
            (animation) =>
              animation instanceof CSSAnimation &&
              animation.animationName === "composer-working" &&
              animation.playState === "running",
          ).length,
    );
  await expect.poll(running).toBe(1);
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "hidden",
    });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect(dock).toBeHidden();
  expect(await running()).toBe(0);
  await page.waitForTimeout(150);
  expect(await running()).toBe(0);
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "visible",
    });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect(dock).toBeVisible();
  await expect.poll(running).toBe(1);
});
