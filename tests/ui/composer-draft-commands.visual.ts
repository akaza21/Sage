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
  commands: unknown[] | null;
  catalog: string[] | null | undefined;
  ran: string[];
  sent: unknown[];
  sessions: number;
}

/** The draft fixture with curated built-ins and a session catalog that is
 * still unreported (revision 0) unless `catalog` is given; `commands` is a
 * reported list the draft offers in place of the built-ins. */
async function openDraft(
  page: Page,
  builtins: string[],
  catalog: string[] | null = null,
  commands: unknown[] | null = null,
) {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(
    async ([names, reported, rows]) => {
      const { installDraftPlusFixture } =
        await import("/tests/ui/fixtures/draftPlusFixture.ts");
      installDraftPlusFixture();
      const fixture = (window as never as { draftPlus: Fixture }).draftPlus;
      fixture.builtins = names;
      fixture.catalog = reported;
      fixture.commands = rows;
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const state = useWorkbenchStore.getState();
      await state.openNewChatDraft(state.project!.id);
    },
    [builtins, catalog, commands] as const,
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
  // A bare `/name` opens the draft's `/` menu, where Enter picks; close it.
  if (/^\/\S*$/u.test(text)) {
    await expect(draft.page().getByRole("listbox")).toBeVisible();
    await field.press("Escape");
    await expect(draft.page().getByRole("listbox")).toHaveCount(0);
  }
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
  await page.evaluate(() => {
    (window as never as { draftPlus: { picks: string[] } }).draftPlus.picks = [
      "shot.png",
    ];
  });
  await opened.menu.getByRole("menuitem", { name: "Attach" }).click();
  await page
    .getByRole("menu", { name: "Attach" })
    .getByRole("menuitem", { name: "Files…" })
    .click();
  await expect(
    opened.draft.getByRole("list", { name: "Attached context" }),
  ).toBeVisible();
  await sendFirst(opened, "/fast");
  await expect(
    opened.chat.getByText(
      "Commands can't include images or files. Nothing was sent.",
    ),
  ).toBeVisible();
  await expect(
    opened.chat.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("/fast");
  expect(await fixture(page)).toMatchObject({ ran: [], sent: 0 });
});

const reported = [
  {
    name: "review-pr",
    description: "Review a pull request",
    inputHint: "<pr>",
    source: "skill",
    placement: { section: "skillsAndCommands" },
  },
  {
    name: "deploy:ship",
    description: "Ship the build",
    inputHint: null,
    source: "plugin",
    sourceLabel: "deploy",
    placement: { section: "skillsAndCommands" },
  },
];

test("before any session the draft offers the harness's reported commands in both menus", async ({
  page,
}) => {
  const { plus, menu, field } = await openDraft(page, ["fast"], null, reported);
  await plus.click();
  await expect(
    menu.getByRole("menuitem", { name: "/review-pr", exact: true }),
  ).toContainText("Review a pull request");
  await expect(
    menu.getByRole("menuitem", { name: "/deploy:ship", exact: true }),
  ).toBeVisible();
  // The reported list replaces the curated built-ins.
  await expect(
    menu.getByRole("menuitem", { name: "/fast", exact: true }),
  ).toHaveCount(0);
  await page.screenshot({ path: `${captureDir}/draft-reported-commands.png` });
  await page.keyboard.press("Escape");
  await field.fill("/rev");
  const option = page.getByRole("option", { name: /\/review-pr/ });
  await expect(option).toBeVisible();
  await page.screenshot({ path: `${captureDir}/draft-slash-commands.png` });
  await option.click();
  await expect(field).toHaveValue("/review-pr ");
  expect(await fixture(page)).toEqual({ ran: [], sent: 0, sessions: 0 });
});

test("a reported command the session no longer lists is refused, not sent as text", async ({
  page,
}) => {
  const opened = await openDraft(page, [], ["compact"], reported);
  await sendFirst(opened, "/review-pr 42");
  await expect(
    opened.chat.getByText(
      "/review-pr is not available in this session. Nothing was sent.",
    ),
  ).toBeVisible();
  await expect(
    opened.chat.getByRole("combobox", { name: "Message agent" }),
  ).toHaveValue("/review-pr 42");
  expect(await fixture(page)).toEqual({ ran: [], sent: 0, sessions: 1 });
});
