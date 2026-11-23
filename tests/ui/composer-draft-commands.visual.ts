import { expect, test, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

const captureDir =
  process.env.SAGE_DRAFT_COMMANDS_CAPTURE_DIR ??
  "output/playwright/composer-draft-commands";

interface Fixture {
  builtins: string[];
  catalog: string[] | null | undefined;
  ran: string[];
  sent: unknown[];
  sessions: number;
}

/** The draft fixture with curated built-ins and a session catalog that is
 * still unreported (revision 0) unless `catalog` is given. */
async function openDraft(
  page: Page,
  builtins: string[],
  catalog: string[] | null = null,
) {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(
    async ([names, reported]) => {
      const { installDraftPlusFixture } =
        await import("/tests/ui/fixtures/draftPlusFixture.ts");
      installDraftPlusFixture();
      const fixture = (window as never as { draftPlus: Fixture }).draftPlus;
      fixture.builtins = names;
      fixture.catalog = reported;
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const state = useWorkbenchStore.getState();
      await state.openNewChatDraft(state.project!.id);
    },
    [builtins, catalog] as const,
  );
  const draft = page.getByRole("region", { name: "New chat draft" });
  await expect(draft).toBeVisible();
  return {
    draft,
    plus: draft.getByRole("button", { name: "Composer options", exact: true }),
    menu: page.getByRole("menu", { name: "Composer options menu" }),
    field: draft.getByRole("combobox", { name: "Message agent" }),
    chat: page.locator(".agent-chat-pane form.composer"),
  };
}

const fixture = (page: Page) =>
  page.evaluate(() => {
    const { ran, sent, sessions } = (window as never as { draftPlus: Fixture })
      .draftPlus;
    return { ran: [...ran], sent: sent.length, sessions };
  });

const reportCatalog = (page: Page, names: string[]) =>
  page.evaluate((reported) => {
    (window as never as { draftPlus: Fixture }).draftPlus.catalog = reported;
  }, names);

async function sendFirst(
  { field, draft }: Awaited<ReturnType<typeof openDraft>>,
  text: string,
) {
  await field.fill(text);
  await field.press("Enter");
  await expect(draft).toHaveCount(0);
}

test("the draft lists its harness's built-in commands, insert-only", async ({
  page,
}) => {
  const { plus, menu, field } = await openDraft(page, ["fast", "agents"]);
  await field.fill("keep it short");
  await plus.click();
  await expect(menu.locator(".composer__add-heading")).toHaveText([
    "Add to message",
    "Skills and commands",
    "Model and access",
  ]);
  const fast = menu.getByRole("menuitem", { name: "/fast", exact: true });
  await expect(fast).toContainText("Built-in");
  await expect(
    menu.getByRole("menuitem", { name: "/agents", exact: true }),
  ).toBeVisible();
  await page.screenshot({ path: `${captureDir}/draft-builtin-commands.png` });
  await fast.click();
  await expect(menu).toHaveCount(0);
  await expect(field).toHaveValue("/fast keep it short");
  await expect(field).toBeFocused();
  expect(await fixture(page)).toEqual({ ran: [], sent: 0, sessions: 0 });
});

test("a first send of a built-in command waits for the catalog, then runs it", async ({
  page,
}) => {
  const opened = await openDraft(page, ["fast"]);
  await sendFirst(opened, "/fast");
  // The composer shows the send working, with Stop.
  await expect(
    opened.chat.getByRole("button", { name: "Cancel turn" }),
  ).toBeEnabled();
  await expect.poll(async () => (await fixture(page)).sessions).toBe(1);
  expect(await fixture(page)).toMatchObject({ ran: [], sent: 0 });
  await page.screenshot({ path: `${captureDir}/first-send-held.png` });
  await reportCatalog(page, ["compact", "fast"]);
  await expect.poll(async () => (await fixture(page)).ran).toEqual(["fast"]);
  expect((await fixture(page)).sent).toBe(0);
});

test("a built-in command the session does not offer sends nothing", async ({
  page,
}) => {
  const opened = await openDraft(page, ["fast"], ["compact"]);
  await sendFirst(opened, "/fast now");
  await expect(
    opened.chat.getByText(
      "/fast is not available in this session. Nothing was sent.",
    ),
  ).toBeVisible();
  await expect(
    opened.chat.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("/fast now");
  await page.screenshot({ path: `${captureDir}/first-send-unavailable.png` });
  expect(await fixture(page)).toEqual({ ran: [], sent: 0, sessions: 1 });
});

test("the first-send hold times out with a clear message", async ({ page }) => {
  const opened = await openDraft(page, ["fast"]);
  await sendFirst(opened, "/fast");
  await expect(
    opened.chat.getByText(
      "/fast did not run: the session's commands took over 5 seconds. Nothing was sent.",
    ),
  ).toBeVisible({ timeout: 10_000 });
  await expect(
    opened.chat.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("/fast");
  expect(await fixture(page)).toMatchObject({ ran: [], sent: 0 });
});

test("Stop cancels the first-send hold", async ({ page }) => {
  const opened = await openDraft(page, ["fast"]);
  await sendFirst(opened, "/fast");
  await expect.poll(async () => (await fixture(page)).sessions).toBe(1);
  await opened.chat.getByRole("button", { name: "Cancel turn" }).click();
  await expect(
    opened.chat.getByText("Stopped before /fast ran. Nothing was sent."),
  ).toBeVisible();
  await reportCatalog(page, ["fast"]);
  await expect(
    opened.chat.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("/fast");
  expect(await fixture(page)).toMatchObject({ ran: [], sent: 0 });
});

const holdList = (page: Page) =>
  page.evaluate(() =>
    (
      window as never as { draftPlus: { holdList(): void } }
    ).draftPlus.holdList(),
  );

test("Stop during an in-flight catalog read sends nothing", async ({
  page,
}) => {
  const opened = await openDraft(page, ["fast"]);
  await holdList(page);
  await sendFirst(opened, "/fast");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as never as { draftPlus: { lists: number } }).draftPlus.lists,
      ),
    )
    .toBeGreaterThan(0);
  await opened.chat.getByRole("button", { name: "Cancel turn" }).click();
  // The read answers with the command only after Stop.
  await reportCatalog(page, ["fast"]);
  await expect(
    opened.chat.getByText("Stopped before /fast ran. Nothing was sent."),
  ).toBeVisible({ timeout: 10_000 });
  expect(await fixture(page)).toMatchObject({ ran: [], sent: 0 });
});

test("a hung catalog read ends the hold at its deadline", async ({ page }) => {
  const opened = await openDraft(page, ["fast"]);
  await holdList(page);
  await sendFirst(opened, "/fast");
  await expect(
    opened.chat.getByText(
      "/fast did not run: the session's commands took over 5 seconds. Nothing was sent.",
    ),
  ).toBeVisible({ timeout: 10_000 });
  await expect(
    opened.chat.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("/fast");
  expect(await fixture(page)).toMatchObject({ ran: [], sent: 0 });
});

test("a built-in command with images sends nothing", async ({ page }) => {
  const opened = await openDraft(page, ["fast"], ["fast"]);
  await opened.plus.click();
  const chooser = page.waitForEvent("filechooser");
  await opened.menu.getByRole("menuitem", { name: "Add images" }).click();
  await (
    await chooser
  ).setFiles({
    name: "shot.png",
    mimeType: "image/png",
    buffer: Buffer.from(
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
      "base64",
    ),
  });
  await sendFirst(opened, "/fast");
  await expect(
    opened.chat.getByText("Commands can't include images. Nothing was sent."),
  ).toBeVisible();
  await expect(
    opened.chat.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("/fast");
  expect(await fixture(page)).toMatchObject({ ran: [], sent: 0 });
});
