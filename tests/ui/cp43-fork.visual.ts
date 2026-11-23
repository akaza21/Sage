import { expect, test, type Page } from "@playwright/test";
import {
  waitForAnimations,
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";

test.use({ deviceScaleFactor: 1 });

async function openForkableAnswer(page: Page) {
  await page.setViewportSize({ width: 1872, height: 1080 });
  const workbench = await openScenario(page, "rich", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/transcriptSelectionFixtures.ts");
    fixture.installTranscriptSelectionFixture(false, true);
  });
  await settleTranscriptAtEnd(page);
  return workbench;
}

test("CP-43: Branch in new chat forks above the parent and links back", async ({
  page,
}) => {
  const workbench = await openForkableAnswer(page);
  const parentTitle = await page
    .locator(".agent-task-header__title")
    .first()
    .innerText();
  const more = page
    .getByRole("group", { name: "Message actions" })
    .getByRole("button", { name: "More actions", exact: true });
  await expect(more).toHaveAttribute("aria-haspopup", "menu");
  await more.click();
  const menu = page.getByRole("menu", { name: "Answer actions" });
  await expect(menu.getByRole("menuitem")).toHaveCount(1);
  const item = menu.getByRole("menuitem", { name: "Branch in new chat" });
  await expect(item).toBeVisible();
  await waitForAnimations(page.locator(".chat-transcript-actions").last());
  await waitForAnimations(menu);
  const menuBox = (await menu.boundingBox())!;
  const triggerBox = (await more.boundingBox())!;
  expect(menuBox.y + menuBox.height).toBeLessThanOrEqual(triggerBox.y);
  await expect(workbench).toHaveScreenshot("answer-more-menu.png");

  await item.click();
  const dialog = page.getByRole("dialog", { name: "Fork chat from here" });
  await expect(dialog).toBeVisible();
  const box = (await dialog.boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(
    Math.abs(box.x + box.width / 2 - viewport.width / 2),
  ).toBeLessThanOrEqual(4);
  expect(
    Math.abs(box.y + box.height / 2 - viewport.height / 2),
  ).toBeLessThanOrEqual(4);
  const options = dialog
    .getByRole("group", { name: "Fork chat from here" })
    .getByRole("button");
  await expect(options).toHaveCount(1);
  await expect(options.first()).toContainText("Fork in this workspace");
  await expect(options.first()).toContainText(
    "Fork from this message in the current workspace",
  );
  await expect(dialog.getByText(/worktree/i)).toHaveCount(0);
  // Codex s2/f0055: a #2a2a2a dialog whose focused first option shows no
  // fill or ring after a pointer open; hover fills it.
  await expect(dialog).toHaveCSS("background-color", "rgb(42, 42, 42)");
  await expect(options.first()).toBeFocused();
  await expect(options.first()).toHaveCSS(
    "background-color",
    "rgba(0, 0, 0, 0)",
  );
  await expect(options.first()).toHaveCSS("outline-style", "none");
  await expect(workbench).toHaveScreenshot("fork-dialog.png");
  await options.first().hover();
  await expect(options.first()).toHaveCSS(
    "background-color",
    "rgb(53, 53, 53)",
  );
  await page.mouse.move(0, 0);

  await page.keyboard.press("ArrowDown");
  await expect(options.first()).not.toHaveAttribute("data-fresh");
  await expect(options.first()).toHaveCSS("outline-style", "solid");
  await page.emulateMedia({ forcedColors: "active" });
  await expect(options.first()).toHaveCSS("outline-style", "solid");
  await expect(options.first()).toHaveCSS("outline-width", "2px");
  await page.emulateMedia({ forcedColors: "none" });

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await more.click();
  await item.click();
  await options.first().click();
  await expect(dialog).toBeHidden();
  await expect(page.locator(".agent-task-header__title").first()).toHaveText(
    `${parentTitle} (2)`,
  );
  // The sidebar lists the fork directly above its parent (Codex s2/f0063).
  const rows = page.getByRole("button", {
    name: new RegExp(`^Open chat ${parentTitle}`),
  });
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText(`${parentTitle} (2)`);
  await expect(rows.nth(1)).toHaveText(parentTitle);
  // Like Codex (s2/f0064-f0066) the fork takes over the current pane alone,
  // never paired with its parent: the parent's history up to the forked
  // answer sits read-only above the divider, and the fork has no forkable
  // answer of its own yet.
  const forkPane = page.getByRole("region", {
    name: `Chat pane ${parentTitle} (2)`,
    exact: true,
  });
  await expect(
    page.getByRole("region", { name: `Chat pane ${parentTitle}`, exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("separator", { name: "Resize chat panes" }),
  ).toHaveCount(0);
  const inherited = forkPane.getByRole("group", {
    name: "History from the parent chat",
  });
  await expect(inherited).toBeVisible();
  await expect(inherited).toContainText("The preceding explanation");
  const link = forkPane.getByRole("button", {
    name: "Continued from chat",
    exact: true,
  });
  await expect(link).toBeVisible();
  const historyBox = (await inherited.boundingBox())!;
  const linkBox = (await link.boundingBox())!;
  expect(historyBox.y + historyBox.height).toBeLessThanOrEqual(linkBox.y);
  await expect(workbench).toHaveScreenshot("fork-thread-divider.png");
  // The inherited answer keeps a usable "…", offering only Copy (s2/f0066).
  await inherited
    .getByRole("button", { name: "More actions", exact: true })
    .last()
    .click();
  await expect(
    page.getByRole("menu", { name: "Answer actions" }).getByRole("menuitem"),
  ).toHaveText(["Copy"]);
  await page.keyboard.press("Escape");

  await link.click();
  await expect(page.locator(".agent-task-header__title").first()).toHaveText(
    parentTitle,
  );
});

test("T11a: an earlier prompt rewinds after confirmation and one retry restores the composer", async ({
  page,
}) => {
  const workbench = await openForkableAnswer(page);
  const prompt = await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/conversationRewindFixture.ts");
    return fixture.installConversationRewindFixture();
  });
  const actions = page.locator(".acp-prompt-actions");
  await expect(actions).toHaveCount(2);
  await actions
    .first()
    .getByRole("button", { name: "Message actions", exact: true })
    .click();
  const menu = page.getByRole("menu", { name: "Prompt actions" });
  await expect(
    menu.getByRole("menuitem", { name: "Rewind here", exact: true }),
  ).toBeVisible();
  await expect(
    menu.getByRole("menuitem", { name: "Fork from here", exact: true }),
  ).toBeVisible();
  await waitForAnimations(menu);
  await expect(workbench).toHaveScreenshot("rewind-prompt-actions.png");
  await menu
    .getByRole("menuitem", { name: "Rewind here", exact: true })
    .click();
  await page.keyboard.press("Escape");
  const trigger = actions
    .first()
    .getByRole("button", { name: "Message actions", exact: true });
  await expect(trigger).toBeFocused();
  await trigger.click();
  await menu
    .getByRole("menuitem", { name: "Rewind here", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Confirm rewind", exact: true })
    .click();
  await expect(
    page.getByText("Harness declined rewind.", { exact: false }),
  ).toHaveCount(1);
  await page
    .getByRole("dialog", { name: "Rewind conversation from here" })
    .getByRole("button", { name: "Retry", exact: true })
    .click();
  await expect(actions).toHaveCount(0);
  await expect(
    page.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue(prompt);
});

async function openFileRewind(
  page: Page,
  files: { files: string[]; conflicts: string[] },
) {
  const workbench = await openForkableAnswer(page);
  await page.evaluate(async (files) => {
    const fixture =
      await import("/tests/ui/fixtures/conversationRewindFixture.ts");
    fixture.installConversationRewindFixture(true, "hot", "all", files);
  }, files);
  const actions = page.locator(".acp-prompt-actions");
  await actions
    .first()
    .getByRole("button", { name: "Message actions", exact: true })
    .click();
  await page
    .getByRole("menu", { name: "Prompt actions" })
    .getByRole("menuitem", { name: "Rewind here", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Rewind conversation from here",
  });
  const sent = () =>
    page.evaluate(
      () => (window as { __rewindControls?: unknown[] }).__rewindControls,
    );
  return { workbench, actions, dialog, sent };
}

/** Dry runs only (a dev effect may repeat one), then exactly one rewind. */
function rewound(controls: unknown[] | undefined, files: { files?: true }) {
  const before = "turn-00000000-0000-4000-8000-000000000001";
  const sent = controls ?? [];
  expect(sent.length).toBeGreaterThan(1);
  expect(sent.slice(0, -1)).toEqual(
    sent.slice(0, -1).map(() => ({ kind: "previewRewind", before })),
  );
  expect(sent.at(-1)).toEqual({ kind: "rewindConversation", before, ...files });
}

test("T11b: a preview lists the files, then files and conversation rewind", async ({
  page,
}) => {
  const { workbench, actions, dialog, sent } = await openFileRewind(page, {
    files: ["src/app.ts", "src/app.test.ts"],
    conflicts: [],
  });
  await expect(dialog.getByRole("listitem")).toHaveText([
    "src/app.ts",
    "src/app.test.ts",
  ]);
  await expect(
    dialog.getByText("Rewind does not undo shell commands", { exact: false }),
  ).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Rewind conversation only" }),
  ).toBeFocused();
  await expect(workbench).toHaveScreenshot("rewind-files-preview.png");
  await dialog
    .getByRole("button", { name: "Rewind files and conversation" })
    .click();
  await expect(actions).toHaveCount(0);
  rewound(await sent(), { files: true });
});

test("T11b: files changed since the agent wrote them refuse a file rewind", async ({
  page,
}) => {
  const { actions, dialog, sent } = await openFileRewind(page, {
    files: ["src/app.ts", "src/app.test.ts"],
    conflicts: ["src/app.ts"],
  });
  await expect(dialog.getByRole("alert")).toContainText(
    "changed since the agent last wrote them",
  );
  await expect(dialog.getByRole("listitem")).toHaveText(["src/app.ts"]);
  await expect(
    dialog.getByRole("button", { name: "Rewind files and conversation" }),
  ).toHaveCount(0);
  await dialog
    .getByRole("button", { name: "Rewind conversation only", exact: true })
    .click();
  await expect(actions).toHaveCount(0);
  rewound(await sent(), {});
});

test("T11d: Undo confirms over Rewind's file preview, and a failed one offers the conversation only", async ({
  page,
}) => {
  await openForkableAnswer(page);
  const failure = await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/conversationRewindFixture.ts");
    fixture.installConversationRewindFixture(true, "hot", "undo", {
      files: ["src/app.ts", "src/app.test.ts"],
      conflicts: [],
    });
    (window as { __failRewindPreview?: boolean }).__failRewindPreview = true;
    return fixture.PREVIEW_FAILURE;
  });
  const surface = page.getByRole("region", { name: "Session activity" });
  const undo = surface.getByRole("button", { name: "Undo last turn" });
  const confirm = surface.getByRole("button", { name: "Confirm undo" });
  const turnId = "turn-00000000-0000-4000-8000-000000000002";
  const undos = async () =>
    (
      (await page.evaluate(
        () => (window as { __rewindControls?: unknown[] }).__rewindControls,
      )) as { kind: string }[]
    ).filter((control) => control.kind === "undoTurn");

  // A failed check holds the file undo and offers the conversation only.
  await undo.click();
  await expect(surface.getByRole("alert")).toContainText(failure);
  await expect(confirm).toBeDisabled();
  await confirm.click({ force: true });
  expect(await undos()).toEqual([]);
  await surface
    .getByRole("button", { name: "Undo conversation only; files stay" })
    .click();
  await expect
    .poll(undos)
    .toEqual([{ kind: "undoTurn", turnId, keepFiles: true }]);

  // Opening it again asks for the preview again; a clean one restores files.
  await page.evaluate(() => {
    (window as { __failRewindPreview?: boolean }).__failRewindPreview = false;
  });
  await undo.click();
  await expect(surface.getByRole("listitem")).toHaveText([
    "src/app.ts",
    "src/app.test.ts",
  ]);
  await expect(surface.getByRole("alert")).toHaveCount(0);
  await confirm.click();
  await expect
    .poll(async () => (await undos()).at(-1))
    .toEqual({
      kind: "undoTurn",
      turnId,
    });
});

test("T22: a cold session's completed turns carry no ellipsis actions row", async ({
  page,
}) => {
  await openForkableAnswer(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/conversationRewindFixture.ts");
    fixture.installConversationRewindFixture(true, "cold");
  });
  const turns = page.locator('.acp-turn[data-turn-state="completed"]');
  await expect(turns).toHaveCount(2);
  await expect(page.locator(".acp-prompt-actions")).toHaveCount(0);
  // Between each prompt and its "Worked" heading nothing but the prompt renders.
  for (const turn of await turns.all())
    await expect(
      turn.locator(".acp-turn__items > .chat-transcript-actions"),
    ).toHaveCount(0);
});

test("T22b: prompt actions reveal on hover and Tab without moving the transcript", async ({
  page,
}) => {
  await openForkableAnswer(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/conversationRewindFixture.ts");
    fixture.installConversationRewindFixture();
  });
  await settleTranscriptAtEnd(page);
  const prompt = page
    .locator(".acp-turn__items > :has(> .acp-prompt-actions)")
    .first();
  const trigger = prompt.getByRole("button", {
    name: "Message actions",
    exact: true,
  });
  await expect(trigger).toHaveCount(1);
  await page.mouse.move(0, 0);
  await expect(prompt.locator(".acp-prompt-actions")).toHaveCSS("opacity", "0");
  const following = page.locator(".acp-turn__items > *, .acp-virtual-turn-row");
  const boxes = () =>
    following.evaluateAll((items) =>
      items.map((item) => item.getBoundingClientRect().toJSON()),
    );
  const before = await boxes();
  await prompt.locator(".acp-message--user").hover();
  await expect(prompt.locator(".acp-prompt-actions")).toHaveCSS("opacity", "1");
  expect(await boxes()).toEqual(before);
  const bubble = (await prompt.locator(".acp-message--user").boundingBox())!;
  const button = (await trigger.boundingBox())!;
  expect(button.y).toBeGreaterThanOrEqual(bubble.y + bubble.height);
  expect(button.x + button.width).toBe(bubble.x + bubble.width);
  await page.mouse.move(0, 0);
  await expect(prompt.locator(".acp-prompt-actions")).toHaveCSS("opacity", "0");
  // Enter from the preceding workbench control using real keyboard navigation.
  await trigger.focus();
  await page.keyboard.press("Shift+Tab");
  await expect(trigger).not.toBeFocused();
  await expect(prompt.locator(".acp-prompt-actions")).toHaveCSS("opacity", "0");
  await page.keyboard.press("Tab");
  await expect(trigger).toBeFocused();
  await expect(prompt.locator(".acp-prompt-actions")).toHaveCSS("opacity", "1");
  expect(await boxes()).toEqual(before);
});

test("T22b: unchanged prompts follow eligibility updates without moving", async ({
  page,
}) => {
  await openForkableAnswer(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/conversationRewindFixture.ts");
    fixture.installConversationRewindFixture();
  });
  const actions = page.locator(".acp-prompt-actions");
  await expect(actions).toHaveCount(2);
  // The earlier prompt, its summary and its answer never move. Offsets are
  // turn-relative: the transcript may scroll as the later turn changes.
  const earlierLayout = () =>
    page
      .locator(".acp-turn")
      .first()
      .evaluate((turn) => {
        const origin = turn.getBoundingClientRect();
        return [
          ...turn.querySelectorAll(
            ".acp-turn__items > :has(> .acp-message--user), .acp-message--user, .chat-transcript-work, .acp-message--assistant",
          ),
        ].map((item) => {
          const box = item.getBoundingClientRect();
          return [box.x - origin.x, box.y - origin.y, box.width, box.height];
        });
      });
  const settled = await earlierLayout();
  for (const availability of [
    "busy",
    "all",
    "cold",
    "all",
    "none",
    "all",
  ] as const) {
    const retained = await page.evaluate(async (availability) => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const { acpProjectionStoreFor } =
        await import("/src/app/acpProjectionRegistry.ts");
      const projection = acpProjectionStoreFor(
        useWorkbenchStore.getState().activeTabId!,
      );
      const state = projection.getState();
      const page = state.page!;
      const first = page.turns[0];
      state.applyAuthoritativePage(state.scope!, {
        ...page,
        projectionRevision: String(
          Number(page.projectionRevision) + 1,
        ) as never,
        turns: page.turns.map((turn, index) =>
          index === 0
            ? turn
            : {
                ...turn,
                terminalAt: availability === "busy" ? null : 1,
                state: availability === "busy" ? "running" : "completed",
              },
        ),
        sessions: page.sessions.map((session) => ({
          ...session,
          residency: availability === "cold" ? "cold" : "hot",
          capabilities: {
            ...session.capabilities,
            rewind:
              availability === "none"
                ? undefined
                : {
                    effects: { conversation: true, files: false },
                    targets: "anyTurn",
                    forkAt: true,
                    onResume: false,
                  },
          },
        })),
      });
      return projection.getState().page!.turns[0] === first;
    }, availability);
    expect(retained).toBe(true);
    await expect(actions).toHaveCount(availability === "all" ? 2 : 0);
    expect(await earlierLayout(), availability).toEqual(settled);
  }
  await expect(page.locator(".acp-turn")).toHaveCount(2);
});

test("T22b: latest-turn actions appear only at the offered undo prompt", async ({
  page,
}) => {
  await openForkableAnswer(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/conversationRewindFixture.ts");
    fixture.installConversationRewindFixture(true, "hot", "latestTurn");
  });
  const turns = page.locator(".acp-turn");
  await expect(turns).toHaveCount(2);
  await expect(turns.first().locator(".acp-prompt-actions")).toHaveCount(0);
  const prompt = turns
    .last()
    .locator(".acp-turn__items > :has(.acp-message--user)");
  await prompt.hover();
  await prompt
    .getByRole("button", { name: "Message actions", exact: true })
    .click();
  const menu = page.getByRole("menu", { name: "Prompt actions" });
  await expect(menu.getByRole("menuitem")).toHaveText(["Rewind here"]);
});

test.describe("T22b: no-hover devices", () => {
  test.use({ hasTouch: true });

  test("prompt actions stay visible without hovering", async ({ page }) => {
    await openForkableAnswer(page);
    await page.evaluate(async () => {
      const fixture =
        await import("/tests/ui/fixtures/conversationRewindFixture.ts");
      fixture.installConversationRewindFixture();
    });
    expect(await page.evaluate(() => matchMedia("(hover: none)").matches)).toBe(
      true,
    );
    await page.mouse.move(0, 0);
    const actions = page.locator(".acp-prompt-actions").first();
    await expect(actions).toHaveCSS("opacity", "1");
    await actions
      .getByRole("button", { name: "Message actions", exact: true })
      .tap();
    await expect(
      page.getByRole("menu", { name: "Prompt actions" }).getByRole("menuitem"),
    ).toHaveText(["Rewind here", "Fork from here"]);
  });
});

test("T11a: a fork-only offer has no in-place rewind and explains the shared folder", async ({
  page,
}) => {
  await openForkableAnswer(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/conversationRewindFixture.ts");
    fixture.installConversationRewindFixture(false);
  });
  const actions = page.locator(".acp-prompt-actions");
  await expect(actions).toHaveCount(2);
  await actions
    .first()
    .getByRole("button", { name: "Message actions", exact: true })
    .click();
  const menu = page.getByRole("menu", { name: "Prompt actions" });
  await expect(
    menu.getByRole("menuitem", { name: "Rewind here", exact: true }),
  ).toHaveCount(0);
  await menu
    .getByRole("menuitem", { name: "Fork from here", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Fork chat from here" }),
  ).toContainText("This chat shares the project folder.");
});

test("T11a: message actions load only after their menu opens", async ({
  page,
}) => {
  const requested = new Set<string>();
  const feature =
    /\/src\/features\/chat\/(?:ConversationActions|ForkChat|RewindChatDialog|UserMessageActions|ForkHistory)\.tsx?(?:\?|$)/;
  page.on("request", (request) => {
    const path = new URL(request.url()).pathname;
    if (feature.test(path)) requested.add(path);
  });
  await openForkableAnswer(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/conversationRewindFixture.ts");
    fixture.installConversationRewindFixture();
  });
  const actions = page.locator(".acp-prompt-actions");
  await expect(actions).toHaveCount(2);
  expect([...requested]).toEqual([]);
  await actions
    .first()
    .getByRole("button", { name: "Message actions", exact: true })
    .click();
  await expect(
    page.getByRole("menuitem", { name: "Rewind here", exact: true }),
  ).toBeVisible();
  expect(requested.has("/src/features/chat/UserMessageActions.tsx")).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu", { name: "Prompt actions" })).toHaveCount(
    0,
  );
  await expect(
    actions
      .first()
      .getByRole("button", { name: "Message actions", exact: true }),
  ).toBeFocused();
});
