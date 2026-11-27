import { expect, test, type Page } from "@playwright/test";
import type { AvailableAcpCommand } from "../../src/services/acpCommandsDesktop";
import { installNativeCommands } from "./fixtures/composerCommands";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

const captureDir =
  process.env.SAGE_DRAFT_PLUS_CAPTURE_DIR ??
  "output/playwright/composer-draft-plus";

// A 1×1 transparent PNG.
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
  "base64",
);

interface Captured {
  creates: number;
  uploads: string[];
  sessions: number;
  sent: {
    prompt: string;
    attachedContext?: { kind: string; label: string }[];
  }[];
  access: { mode?: string }[];
}

const captured = (page: Page) =>
  page.evaluate(() => {
    const { creates, uploads, sessions, sent, access } = (
      window as never as { draftPlus: Captured }
    ).draftPlus;
    return { creates, uploads, sessions, sent, access };
  });

const control = (page: Page, step: string) =>
  page.evaluate((name) => {
    const fixture = (
      window as never as {
        draftPlus: Record<string, unknown> & {
          uploadHold: { release(): void } | null;
          pageHold: { release(): void } | null;
        };
      }
    ).draftPlus;
    if (name === "holdUploads" || name === "holdPage")
      (fixture[name] as () => void)();
    else if (name === "releaseUploads") fixture.uploadHold?.release();
    else if (name === "releasePage") fixture.pageHold?.release();
    else fixture.failUpload = name;
  }, step);

async function openDraft(page: Page, before?: () => Promise<void>) {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const { installDraftPlusFixture } =
      await import("/tests/ui/fixtures/draftPlusFixture.ts");
    installDraftPlusFixture();
  });
  await before?.();
  await reopenDraft(page);
  const draft = page.getByRole("region", { name: "New chat draft" });
  await expect(draft).toBeVisible();
  return {
    draft,
    plus: draft.getByRole("button", { name: "Composer options", exact: true }),
    menu: page.getByRole("menu", { name: "Composer options menu" }),
    chips: draft.getByRole("list", { name: "Attached context" }),
    field: draft.getByRole("combobox", { name: "Message agent" }),
  };
}

const reopenDraft = (page: Page, projectless = false) =>
  page.evaluate(async (noProject) => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    await state.openNewChatDraft(noProject ? null : state.project!.id);
  }, projectless);

async function attach(
  page: Page,
  { plus, menu, chips }: Awaited<ReturnType<typeof openDraft>>,
  ...names: string[]
) {
  await plus.click();
  await page.evaluate((picked) => {
    (window as never as { draftPlus: { picks: string[] } }).draftPlus.picks =
      picked;
  }, names);
  await menu.getByRole("menuitem", { name: "Attach" }).click();
  await page
    .getByRole("menu", { name: "Attach" })
    .getByRole("menuitem", { name: "Files…" })
    .click();
  await expect(chips.getByRole("listitem")).toHaveCount(names.length);
}

test("the new-chat draft's + menu offers only draft-safe sections", async ({
  page,
}) => {
  const { plus, menu } = await openDraft(page);
  await plus.click();
  await expect(menu.locator(".composer__add-heading")).toHaveText([
    "Add to message",
    "Model and access",
  ]);
  await expect(menu.getByRole("menuitem", { name: "Attach" })).toBeVisible();
  await expect(
    menu.getByRole("menuitemcheckbox", { name: "Plan mode" }),
  ).toBeVisible();
  for (const name of ["Work in a project", "Goal", "Compact context", "Usage"])
    await expect(menu.getByRole("menuitem", { name })).toHaveCount(0);
  await page.screenshot({ path: `${captureDir}/draft-plus-menu.png` });
});

test("the draft never shows the previous chat's command catalog", async ({
  page,
}) => {
  const commands: AvailableAcpCommand[] = [
    {
      name: "audit",
      description: "Review a change",
      inputHint: "scope",
      source: "skill",
      placement: { section: "skillsAndCommands" },
    },
  ];
  const { plus, menu } = await openDraft(page, async () => {
    await installNativeCommands(page, commands);
    const chatPlus = page
      .locator("form.composer")
      .first()
      .getByRole("button", { name: "Composer options", exact: true });
    await chatPlus.click();
    await expect(
      page.getByRole("menuitem", { name: "/audit", exact: true }),
    ).toBeVisible();
    await page.keyboard.press("Escape");
  });
  const requests = () =>
    page.evaluate(
      () =>
        (window as never as { __slashCommands: { requests: unknown[] } })
          .__slashCommands.requests.length,
    );
  const before = await requests();
  await plus.click();
  await expect(menu.locator(".composer__add-heading")).toHaveText([
    "Add to message",
    "Model and access",
  ]);
  await expect(menu.getByRole("menuitem", { name: /^\// })).toHaveCount(0);
  expect(await requests()).toBe(before);
});

test("Plan mode in the draft's + menu drives its picker and reaches the session", async ({
  page,
}) => {
  const { draft, plus, menu, field } = await openDraft(page);
  const access = draft.getByRole("button", { name: "Access options" });
  await expect(access).not.toHaveText("Plan");
  await plus.click();
  await menu.getByRole("menuitemcheckbox", { name: "Plan mode" }).click();
  await expect(menu).toHaveCount(0);
  await expect(access).toHaveText("Plan");
  await expect(access).toHaveAttribute("data-selected-id", "plan");
  await plus.click();
  await expect(
    menu.getByRole("menuitemcheckbox", { name: "Plan mode" }),
  ).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Escape");
  await field.fill("Plan this");
  await field.press("Enter");
  await expect.poll(async () => (await captured(page)).sent.length).toBe(1);
  const { sessions, access: writes } = await captured(page);
  expect(sessions).toBe(1);
  expect(writes.map(({ mode }) => mode)).toEqual(["plan"]);
});

test("an image attached on the draft reaches the first turn once", async ({
  page,
}) => {
  const opened = await openDraft(page);
  const { draft, chips, field } = opened;
  await attach(page, opened, "shot.png");
  await expect(chips.getByRole("listitem")).toHaveText(["Image · shot.png"]);
  await expect(chips.locator("span[title]")).toHaveAttribute("title", "1 × 1");
  await field.fill("Describe this");
  await field.press("Enter");

  await expect(draft).toHaveCount(0);
  await expect.poll(async () => (await captured(page)).sent.length).toBe(1);
  const { sent, uploads, creates } = await captured(page);
  expect(sent[0].prompt).toBe("Describe this");
  expect(
    sent[0].attachedContext?.map(({ kind, label }) => ({ kind, label })),
  ).toEqual([{ kind: "image", label: "shot.png" }]);
  expect(uploads).toEqual(["shot.png"]);
  expect(creates).toBe(1);
  // The chat's composer gave its images to that turn; a fresh draft is empty.
  await expect(
    page.locator(".agent-chat-pane form.composer").getByRole("list", {
      name: "Attached context",
    }),
  ).toHaveCount(0);
  await reopenDraft(page);
  await expect(
    page.getByRole("region", { name: "New chat draft" }),
  ).toBeVisible();
  await expect(
    page
      .getByRole("region", { name: "New chat draft" })
      .getByRole("list", { name: "Attached context" }),
  ).toHaveCount(0);
  expect((await captured(page)).sent).toHaveLength(1);
});

test("a file staged on the draft goes with the first turn once", async ({
  page,
}) => {
  const opened = await openDraft(page, () =>
    page.evaluate(async () => {
      const { installDraftFilesFixture } =
        await import("/tests/ui/fixtures/draftFilesFixture.ts");
      installDraftFilesFixture();
    }),
  );
  const { draft, plus, menu, field } = opened;
  const files = (scope: typeof draft) =>
    scope.getByRole("list", { name: "Attachments for the next message" });
  await plus.click();
  await menu.getByRole("menuitem", { name: "Attach" }).click();
  await page
    .getByRole("menu", { name: "Attach" })
    .getByRole("menuitem", { name: "Files…" })
    .click();
  await expect(files(draft).getByRole("listitem")).toHaveText([
    "notes.txt · 2 KB",
  ]);
  await page.screenshot({ path: `${captureDir}/draft-files-chip.png` });
  await field.fill("Read this");
  await field.press("Enter");

  await expect(draft).toHaveCount(0);
  await expect.poll(async () => (await captured(page)).sent.length).toBe(1);
  const turn = await page.evaluate(() => {
    const { draftPlus, draftFiles } = window as never as {
      draftPlus: { sent: { prompt: string; staged?: { revision: string } }[] };
      draftFiles: { handOffs: { from: string; to: string }[] };
    };
    return { sent: draftPlus.sent, handOffs: draftFiles.handOffs };
  });
  expect(turn.sent[0].prompt).toBe("Read this");
  expect(turn.sent[0].staged?.revision).toEqual(expect.any(String));
  expect(turn.handOffs).toEqual([{ from: "draft", to: "chat" }]);
  // The turn consumed the batch: the chat shows none, a fresh draft none.
  await expect(
    files(page.locator(".agent-chat-pane form.composer")),
  ).toHaveCount(0);
  await reopenDraft(page);
  const fresh = page.getByRole("region", { name: "New chat draft" });
  await expect(fresh).toBeVisible();
  await expect(files(fresh)).toHaveCount(0);
  expect((await captured(page)).sent).toHaveLength(1);
});

test("a cancelled first send keeps the images in the created chat", async ({
  page,
}) => {
  const opened = await openDraft(page);
  await attach(page, opened, "shot.png");
  await control(page, "holdPage");
  await opened.field.fill("Keep this");
  await opened.field.press("Enter");
  await expect(opened.draft).toHaveCount(0);
  // Editor mode cancels the automatic first send.
  await page.locator("label.title-bar__mode", { hasText: "Editor" }).click();
  await page.locator("label.title-bar__mode", { hasText: "Agent" }).click();
  expect(
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      return useWorkbenchStore.getState().pendingFirstSend;
    }),
  ).toBeNull();
  await control(page, "releasePage");
  const composer = page.locator(".agent-chat-pane form.composer");
  await expect(
    composer.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("Keep this");
  await expect(
    composer
      .getByRole("list", { name: "Attached context" })
      .getByRole("listitem"),
  ).toHaveText(["Image · shot.png"]);
  expect((await captured(page)).sent).toHaveLength(0);
});

test("a failed upload keeps every image on the draft for a retargeted retry", async ({
  page,
}) => {
  const opened = await openDraft(page);
  await attach(page, opened, "first.png", "second.png");
  await control(page, "second.png");
  await opened.field.fill("Both images");
  await opened.field.press("Enter");
  await expect(
    opened.draft.getByText("Could not upload second.png."),
  ).toBeVisible();
  // All or nothing: the draft keeps both, and the chat's composer none.
  await expect(opened.chips.getByRole("listitem")).toHaveText([
    "Image · first.png",
    "Image · second.png",
  ]);
  const chatId = await page.evaluate(
    () =>
      (window as never as { draftPlus: { chatId: string } }).draftPlus.chatId,
  );
  expect(
    await page.evaluate(async (id) => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      return useWorkbenchStore.getState().chatUiById[id]?.attachedContext
        .length;
    }, chatId),
  ).toBe(0);
  await reopenDraft(page, true);
  await expect(opened.chips.getByRole("listitem")).toHaveCount(2);
  await opened.field.press("Enter");
  await expect.poll(async () => (await captured(page)).sent.length).toBe(1);
  const { sent, creates } = await captured(page);
  expect(sent[0].attachedContext?.map(({ label }) => label)).toEqual([
    "first.png",
    "second.png",
  ]);
  expect(creates).toBe(1);
});

test("the draft sends once while its images upload", async ({ page }) => {
  const opened = await openDraft(page);
  await attach(page, opened, "shot.png");
  await control(page, "holdUploads");
  await opened.field.fill("Once");
  await opened.field.press("Enter");
  await expect(opened.field).toBeDisabled();
  await expect(
    opened.draft.getByRole("button", { name: /send/i }),
  ).toBeDisabled();
  await page.keyboard.press("Enter");
  await control(page, "releaseUploads");
  await expect.poll(async () => (await captured(page)).sent.length).toBe(1);
  const { sent, uploads, creates } = await captured(page);
  expect(sent).toHaveLength(1);
  expect(uploads).toEqual(["shot.png"]);
  expect(creates).toBe(1);
});

test("leaving the created chat mid-upload hands off without sending", async ({
  page,
}) => {
  const opened = await openDraft(page);
  await attach(page, opened, "shot.png");
  await control(page, "holdUploads");
  await opened.field.fill("Later");
  await opened.field.press("Enter");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as never as { draftPlus: { started: number } }).draftPlus
            .started,
      ),
    )
    .toBe(1);
  const previous = await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    const id = state.tabs.find(
      (tab) =>
        tab.kind === "chat" &&
        tab.id !==
          (window as never as { draftPlus: { chatId: string } }).draftPlus
            .chatId,
    )!.id;
    await state.activateTab(id);
    return id;
  });
  await control(page, "releaseUploads");
  const handed = () =>
    page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const state = useWorkbenchStore.getState();
      const id = (window as never as { draftPlus: { chatId: string } })
        .draftPlus.chatId;
      const entry = state.chatUiById[id];
      return {
        draft: state.newChatDraft,
        active: state.activeTabId,
        text: entry?.composerText,
        images: entry?.attachedContext.map((context) =>
          context.kind === "image" ? context.label : context.kind,
        ),
      };
    });
  await expect.poll(async () => (await handed()).draft).toBeNull();
  expect(await handed()).toEqual({
    draft: null,
    active: previous,
    text: "Later",
    images: ["shot.png"],
  });
  expect((await captured(page)).sent).toHaveLength(0);
});

test("images dropped just before Send all go in that send", async ({
  page,
}) => {
  const { field } = await openDraft(page);
  await field.fill("Right away");
  await field.evaluate((textbox, bytes) => {
    const form = (textbox as HTMLTextAreaElement).form!;
    for (const name of ["first.png", "second.png"]) {
      const transfer = new DataTransfer();
      transfer.items.add(
        new File([Uint8Array.from(atob(bytes), (c) => c.charCodeAt(0))], name, {
          type: "image/png",
        }),
      );
      form.dispatchEvent(
        new DragEvent("drop", {
          dataTransfer: transfer,
          bubbles: true,
          cancelable: true,
        }),
      );
    }
    form.requestSubmit();
  }, PNG.toString("base64"));
  await expect.poll(async () => (await captured(page)).sent.length).toBe(1);
  expect(
    (await captured(page)).sent[0].attachedContext?.map(({ label }) => label),
  ).toEqual(["first.png", "second.png"]);
});

test("a handoff that fails after uploading takes its images back for the retry", async ({
  page,
}) => {
  const opened = await openDraft(page);
  await attach(page, opened, "shot.png");
  await control(page, "holdUploads");
  await opened.field.fill("Retry me");
  await opened.field.press("Enter");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as never as { draftPlus: { started: number } }).draftPlus
            .started,
      ),
    )
    .toBe(1);
  // Opening a file, diff or browser tab takes the active tab without
  // cancelling the pending first send, so the handoff then fails.
  const chatId = await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    const created = (window as never as { draftPlus: { chatId: string } })
      .draftPlus.chatId;
    useWorkbenchStore.setState({
      activeTabId: state.tabs.find(
        (tab) => tab.kind === "chat" && tab.id !== created,
      )!.id,
    });
    return created;
  });
  await control(page, "releaseUploads");
  await expect(
    opened.draft.getByText(
      "Could not open the created chat. Retry to continue.",
    ),
  ).toBeVisible();
  await expect(opened.chips.getByRole("listitem")).toHaveText([
    "Image · shot.png",
  ]);
  expect(
    await page.evaluate(async (id) => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      return useWorkbenchStore.getState().chatUiById[id]?.attachedContext
        .length;
    }, chatId),
  ).toBe(0);
  await opened.field.press("Enter");
  await expect.poll(async () => (await captured(page)).sent.length).toBe(1);
  const { sent, creates } = await captured(page);
  expect(sent[0].attachedContext?.map(({ label }) => label)).toEqual([
    "shot.png",
  ]);
  expect(creates).toBe(1);
});

test("moving the draft to another project keeps its images", async ({
  page,
}) => {
  const opened = await openDraft(page);
  await attach(page, opened, "shot.png");
  await expect(opened.chips.getByRole("listitem")).toHaveCount(1);
  await reopenDraft(page, true);
  await expect(opened.chips.getByRole("listitem")).toHaveText([
    "Image · shot.png",
  ]);
  expect(
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      return useWorkbenchStore.getState().newChatDraft?.targetProjectId;
    }),
  ).toBeNull();
});
