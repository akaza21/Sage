import { expect, test, type Page } from "@playwright/test";
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

async function openDraft(page: Page) {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const { installDraftPlusFixture } =
      await import("/tests/ui/fixtures/draftPlusFixture.ts");
    installDraftPlusFixture();
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    await state.openNewChatDraft(state.project!.id);
  });
  const draft = page.getByRole("region", { name: "New chat draft" });
  await expect(draft).toBeVisible();
  return {
    draft,
    plus: draft.getByRole("button", { name: "Composer options", exact: true }),
    menu: page.getByRole("menu", { name: "Composer options menu" }),
  };
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

test("Plan mode in the draft's + menu drives the draft's access picker", async ({
  page,
}) => {
  const { draft, plus, menu } = await openDraft(page);
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
});

test("an image attached on the draft reaches the first turn once", async ({
  page,
}) => {
  const { draft, plus, menu } = await openDraft(page);
  await plus.click();
  const chooser = page.waitForEvent("filechooser");
  await menu.getByRole("menuitem", { name: "Add images" }).click();
  await (
    await chooser
  ).setFiles({ name: "shot.png", mimeType: "image/png", buffer: PNG });
  const chips = draft.getByRole("list", { name: "Attached context" });
  await expect(chips.getByRole("listitem")).toHaveText(["Image · shot.png"]);
  const field = draft.getByRole("combobox", { name: "Message agent" });
  await field.fill("Describe this");
  await field.press("Enter");

  await expect(draft).toHaveCount(0);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as never as { draftPlusSent: unknown[] }).draftPlusSent
            .length,
      ),
    )
    .toBe(1);
  const turn = await page.evaluate(
    () =>
      (
        window as never as {
          draftPlusSent: {
            prompt: string;
            attachedContext?: { kind: string; label: string }[];
          }[];
        }
      ).draftPlusSent[0],
  );
  expect(turn.prompt).toBe("Describe this");
  expect(
    turn.attachedContext?.map(({ kind, label }) => ({ kind, label })),
  ).toEqual([{ kind: "image", label: "shot.png" }]);
  expect(
    await page.evaluate(() =>
      (
        window as never as { draftPlusUploads: () => number }
      ).draftPlusUploads(),
    ),
  ).toBe(1);

  // The chat's composer gave its images to that turn, and a fresh draft
  // starts empty.
  await expect(
    page.locator(".agent-chat-pane form.composer").getByRole("list", {
      name: "Attached context",
    }),
  ).toHaveCount(0);
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    await state.openNewChatDraft(state.project!.id);
  });
  const fresh = page.getByRole("region", { name: "New chat draft" });
  await expect(fresh).toBeVisible();
  await expect(
    fresh.getByRole("list", { name: "Attached context" }),
  ).toHaveCount(0);
  expect(
    await page.evaluate(
      () =>
        (window as never as { draftPlusSent: unknown[] }).draftPlusSent.length,
    ),
  ).toBe(1);
});
