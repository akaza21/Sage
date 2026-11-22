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

async function setDocumentVisibility(page: Page, value: "hidden" | "visible") {
  await page.evaluate((visibilityState) => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: visibilityState,
    });
    document.dispatchEvent(new Event("visibilitychange"));
  }, value);
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

async function dictateDraft(page: Page, keyboard = false) {
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
  const start = dock.getByRole("button", { name: "Start dictation" });
  if (keyboard) {
    await start.focus();
    await page.keyboard.press("Enter");
  } else {
    await start.click();
  }
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
  return {
    prompt,
    stop,
    send: dock.getByRole("button", { name: "Send message", exact: true }),
    endRecognition: () =>
      page.evaluate(() => {
        const { fakeRecognition } = window as unknown as {
          fakeRecognition: { onend: () => void };
        };
        fakeRecognition.onend();
      }),
    submits: () =>
      page.evaluate(() => (window as unknown as { submits: number }).submits),
  };
}

for (const endWhilePressed of [false, true]) {
  test(`Stop dictation keeps the spoken draft unsent${endWhilePressed ? " when recognition ends during the press" : ""}`, async ({
    page,
  }) => {
    const { prompt, stop, send, endRecognition, submits } =
      await dictateDraft(page);
    if (endWhilePressed) {
      // Press the button padding: its icon is replaced when recognition ends.
      const box = await stop.boundingBox();
      expect(box).not.toBeNull();
      await stop.hover({ position: { x: 4, y: box!.height / 2 } });
      await page.mouse.down();
      await endRecognition();
      await expect(send).toBeVisible();
      await page.mouse.up();
    } else {
      await stop.click();
    }
    await expect(send).toBeVisible();
    await expect(prompt).toHaveValue("Review the composer");
    expect(await submits()).toBe(0);
  });
}

for (const end of ["Enter on Stop", "recognition end"] as const) {
  test(`keyboard focus moves from Stop dictation to Send (${end})`, async ({
    page,
  }) => {
    // A keyboard user starts dictation on the control, which stays focused.
    const { prompt, stop, send, endRecognition, submits } = await dictateDraft(
      page,
      true,
    );
    await expect(stop).toBeFocused();
    if (end === "Enter on Stop") await page.keyboard.press("Enter");
    else await endRecognition();
    await expect(send).toBeFocused();
    await expect(prompt).toHaveValue("Review the composer");
    expect(await submits()).toBe(0);
  });
}

test("ending dictation leaves focus where it was when the control was not focused", async ({
  page,
}) => {
  const { prompt, send, endRecognition } = await dictateDraft(page);
  await prompt.focus();
  await endRecognition();
  await expect(send).toBeVisible();
  await expect(prompt).toBeFocused();
});

test("holding Enter on Stop dictation never repeats onto Send", async ({
  page,
}) => {
  const { prompt, stop, send, submits } = await dictateDraft(page, true);
  await expect(stop).toBeFocused();
  await page.keyboard.down("Enter");
  await expect(send).toBeFocused();
  // Further keydowns while the key stays down are auto-repeats.
  await page.keyboard.down("Enter");
  await page.keyboard.down("Enter");
  await page.keyboard.up("Enter");
  await expect(prompt).toHaveValue("Review the composer");
  expect(await submits()).toBe(0);
});

test("a control focused before the document hid does not take focus back", async ({
  page,
}) => {
  const dock = await openComposer(page);
  await dock
    .getByRole("combobox", { name: "Message agent" })
    .fill("Review the composer");
  const send = dock.getByRole("button", { name: "Send message", exact: true });
  await send.focus();
  await setDocumentVisibility(page, "hidden");
  await expect(dock).toBeHidden();
  const editor = page.getByRole("radio", { name: "Editor", exact: true });
  await editor.focus();
  await setDocumentVisibility(page, "visible");
  await expect(send).toBeVisible();
  await expect(editor).toBeFocused();
});

test("keyboard focus on Cancel returns to the prompt when the turn ends", async ({
  page,
}) => {
  const dock = await openComposer(page, true);
  const cancel = dock.getByRole("button", { name: "Cancel turn" });
  await cancel.focus();
  await expect(cancel).toBeFocused();
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const id = useWorkbenchStore.getState().activeTabId!;
    const projection = acpProjectionStoreFor(id).getState();
    projection.applyAuthoritativePage(projection.scope!, {
      ...projection.page!,
      turns: projection.page!.turns.map((turn) => ({
        ...turn,
        state: "cancelled" as const,
        terminalAt: turn.updatedAt,
      })),
    });
  });
  await expect(cancel).toBeHidden();
  await expect(
    dock.getByRole("combobox", { name: "Message agent" }),
  ).toBeFocused();
});

test("Send submits the typed draft exactly once", async ({ page }) => {
  await page.addInitScript(() => {
    let submits = 0;
    document.addEventListener("submit", () => (submits += 1), true);
    Object.defineProperty(window, "submits", { get: () => submits });
  });
  const dock = await openComposer(page);
  await dock
    .getByRole("combobox", { name: "Message agent" })
    .fill("Review the composer");
  // The form submits synchronously within the click it activates.
  await dock.getByRole("button", { name: "Send message", exact: true }).click();
  expect(
    await page.evaluate(
      () => (window as unknown as { submits: number }).submits,
    ),
  ).toBe(1);
});

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
  await setDocumentVisibility(page, "hidden");
  await expect(dock).toBeHidden();
  expect(await running()).toBe(0);
  // Two more rendered frames: nothing may restart the border while hidden.
  await page.evaluate(
    () =>
      new Promise((done) =>
        requestAnimationFrame(() => requestAnimationFrame(done)),
      ),
  );
  expect(await running()).toBe(0);
  await setDocumentVisibility(page, "visible");
  await expect(dock).toBeVisible();
  await expect.poll(running).toBe(1);
});
