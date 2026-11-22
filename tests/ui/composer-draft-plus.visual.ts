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
const png = (name: string) => ({ name, mimeType: "image/png", buffer: PNG });

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
  const chooser = page.waitForEvent("filechooser");
  await menu.getByRole("menuitem", { name: "Add images" }).click();
  await (await chooser).setFiles(names.map(png));
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
  await expect(
    menu.getByRole("menuitem", { name: "Add images" }),
  ).toBeVisible();
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

test("a failed upload keeps the remaining images on the draft", async ({
  page,
}) => {
  const opened = await openDraft(page);
  await attach(page, opened, "first.png", "second.png");
  await expect(opened.chips.getByRole("listitem")).toHaveCount(2);
  await control(page, "second.png");
  await opened.field.fill("Both images");
  await opened.field.press("Enter");
  await expect(
    opened.draft.getByText("Could not upload second.png."),
  ).toBeVisible();
  await expect(opened.chips.getByRole("listitem")).toHaveText([
    "Image · second.png",
  ]);
  await opened.field.press("Enter");
  await expect.poll(async () => (await captured(page)).sent.length).toBe(1);
  const { sent, uploads, creates } = await captured(page);
  expect(sent[0].attachedContext?.map(({ label }) => label)).toEqual([
    "first.png",
    "second.png",
  ]);
  expect(uploads).toEqual(["first.png", "second.png"]);
  expect(creates).toBe(1);
});

test("the draft cannot send twice while its images upload", async ({
  page,
}) => {
  const opened = await openDraft(page);
  await attach(page, opened, "shot.png");
  await control(page, "holdUploads");
  await opened.field.fill("Once");
  await opened.field.press("Enter");
  await expect(opened.field).toBeDisabled();
  await expect(
    opened.draft.getByRole("button", { name: /send/i }),
  ).toBeDisabled();
  await control(page, "releaseUploads");
  await expect.poll(async () => (await captured(page)).sent.length).toBe(1);
  const { uploads, creates } = await captured(page);
  expect(uploads).toEqual(["shot.png"]);
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
