import { expect, test, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";
import { popoverProfiles } from "./fixtures/popoverProfiles";

async function openChrome(
  page: Page,
  harness: string,
  width: number,
  state = "transcript",
  status = "Completed",
  tone = "idle",
  harnessStatus = "Live",
) {
  await page.setViewportSize({ width, height: 1000 });
  await openScenario(
    page,
    state === "home-split" ? "agent-dual" : "agent-single",
    "agent",
  );
  await settleChatProjections(page);
  await page.evaluate(
    async ({ harness, state, status, tone, harnessStatus }) => {
      const { installChatChromeFixture } =
        await import("/tests/ui/fixtures/chatChromeFixture.ts");
      const turnState =
        status === "Queued"
          ? "queued"
          : tone === "working"
            ? "running"
            : tone === "waiting"
              ? "waitingApproval"
              : tone === "failed"
                ? "failed"
                : "completed";
      installChatChromeFixture(
        harness,
        state.startsWith("home"),
        turnState,
        harnessStatus === "Sign in required",
      );
    },
    { harness, state, status, tone, harnessStatus },
  );
  await chooseWindowMode(page, "Editor");
  await expect(page.locator(".agent-task-header")).toHaveCount(1);
  await expect(page.locator(".agent-task-header")).toBeHidden();
  await chooseWindowMode(page, "Agent");
  await settleChatProjections(page);
  const profile =
    popoverProfiles[
      ["codex", "claude-code", "pi", "opencode", "oh-my-pi"].indexOf(harness)
    ];
  const composer = page.locator("form.composer").first();
  // Sign-in discovery has no session owning the model, so no confirmed
  // model or session controls are advertised yet.
  if (harnessStatus !== "Sign in required") {
    await expect(composer.locator(".composer__model-label")).toHaveText(
      `${profile.name} confirmed model`,
    );
    await expect(composer.locator(".composer__effort-picker")).toHaveCount(
      profile.effort ? 1 : 0,
    );
    await expect(
      composer.getByRole("button", { name: "Access options" }),
    ).toHaveCount(profile.access.length ? 1 : 0);
  }
  await page.evaluate(() => document.fonts.ready);
}

async function chooseWindowMode(page: Page, mode: "Agent" | "Editor") {
  const radio = page.getByRole("radio", { name: mode, exact: true });
  await radio.locator("..").click();
  await expect(radio).toBeChecked();
}

test("transcript replacement keeps following through a queued old-layout scroll", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 1000 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const transcript = page.locator(".message-list");
  const remaining = () =>
    transcript.evaluate(
      (element) =>
        element.scrollHeight - element.clientHeight - element.scrollTop,
    );
  await expect.poll(remaining).toBeLessThanOrEqual(1);
  const revisions = await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/chatChromeReplacementRace.ts");
    return fixture.replaceChatChromeWithQueuedScroll();
  });
  await expect.poll(remaining).toBeLessThanOrEqual(1);
  expect(revisions).toEqual({ before: "42", after: "43" });
});

async function expectSingleHeaderRow(page: Page, width: number) {
  await expect(page.locator(".agent-task-header__title")).toBeVisible();
  const header = page.locator(".agent-task-header");
  await expect(header).toBeVisible();
  const geometry = await header.evaluate((element) => {
    const headerBox = element.getBoundingClientRect();
    const children = [...element.children]
      .map((child) => child.getBoundingClientRect())
      .filter((box) => box.width > 0 && box.height > 0);
    const title = element.querySelector(".agent-task-header__title")!;
    return {
      height: headerBox.height,
      oneRow: children.every(
        (child) =>
          Math.abs(
            child.top +
              child.height / 2 -
              (headerBox.top + headerBox.height / 2),
          ) <= 1,
      ),
      contained: children.every(
        (child) =>
          child.left >= headerBox.left && child.right <= headerBox.right,
      ),
      titleWidth: title.getBoundingClientRect().width,
      truncated: title.scrollWidth > title.clientWidth,
    };
  });
  expect(geometry.height).toBe(44);
  expect(geometry.oneRow).toBe(true);
  expect(geometry.contained).toBe(true);
  expect(geometry.titleWidth).toBeGreaterThanOrEqual(24);
  if (width < 1888) expect(geometry.truncated).toBe(true);
}

for (const harness of ["codex", "claude-code", "pi", "opencode", "oh-my-pi"]) {
  for (const width of [1888, 720, 480]) {
    test(`${harness} header and Archived row at ${width}`, async ({ page }) => {
      await openChrome(page, harness, width);
      await expectSingleHeaderRow(page, width);
      if (width <= 600)
        await page
          .getByRole("button", { name: "Show projects and chats" })
          .click();
      const archived = page.getByRole("button", { name: "Archived chats: 1" });
      await expect(archived).toHaveText("Archived 1");
      await expect(archived).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
      await expect(archived).toHaveCSS("border-top-width", "0px");
      expect(
        await archived.evaluate((element) => {
          const channels = getComputedStyle(element)
            .color.match(/[\d.]+/g)!
            .slice(0, 3)
            .map(Number);
          const luminance = (values: number[]) =>
            values
              .map((value) => {
                const channel = value / 255;
                return channel <= 0.04045
                  ? channel / 12.92
                  : ((channel + 0.055) / 1.055) ** 2.4;
              })
              .reduce(
                (total, channel, index) =>
                  total + channel * [0.2126, 0.7152, 0.0722][index],
                0,
              );
          const canvas = getComputedStyle(document.body)
            .backgroundColor.match(/[\d.]+/g)!
            .slice(0, 3)
            .map(Number);
          const light = luminance(channels);
          const dark = luminance(canvas);
          return (
            (Math.max(light, dark) + 0.05) / (Math.min(light, dark) + 0.05)
          );
        }),
      ).toBeGreaterThanOrEqual(4.5);
      const spacing = await archived.evaluate((element) => {
        const [label, count] = element.querySelectorAll("span");
        return (
          count.getBoundingClientRect().left -
          label.getBoundingClientRect().right
        );
      });
      expect(spacing).toBeGreaterThanOrEqual(8);
      await archived.click();
      await expect(archived).toHaveAttribute("aria-expanded", "true");
      await expect(
        page.getByRole("region", { name: "Archived chats" }),
      ).toBeVisible();
      await page
        .getByRole("button", {
          name: "Chat actions for A long chat title that must truncate instead of wrapping the header",
          exact: true,
        })
        // AP-C4: Recents lists every chat, so a pinned chat has two rows.
        .first()
        .click();
      await expect(
        page.getByRole("menuitem", { name: "Close session", exact: true }),
      ).toHaveCount(0);
      await expect(
        page.getByRole("menuitem", { name: "End session", exact: true }),
      ).toHaveCount(0);
      await expectNoOverflow(page);
    });

    test(`${harness} wrapped bubble and round jump control at ${width}`, async ({
      page,
    }) => {
      await openChrome(page, harness, width);
      const bubble = page.locator(".acp-message--user");
      await expect(bubble).toHaveCSS("border-radius", "16px");
      const bubbleBox = await bubble.boundingBox();
      expect(bubbleBox!.height).toBeGreaterThan(64);
      const transcript = page.locator(".message-list");
      await expect
        .poll(() =>
          transcript.evaluate(
            (element) =>
              element.scrollHeight - element.clientHeight - element.scrollTop,
          ),
        )
        .toBeLessThanOrEqual(1);
      await transcript.evaluate((element) => element.scrollTo(0, 0));
      const jump = page.getByRole("button", {
        name: "Jump to latest",
        exact: true,
      });
      await expect(jump).toBeVisible();
      await expect(jump).toHaveText("");
      await expect(jump.locator('svg[aria-hidden="true"]')).toHaveCount(1);
      await expect(jump).toHaveCSS("border-radius", "50%");
      const jumpBox = await jump.boundingBox();
      expect(jumpBox!.width).toBe(28);
      expect(jumpBox!.height).toBe(28);
      const surfaceBox = await page.locator(".chat-surface").boundingBox();
      expect(jumpBox!.x).toBeGreaterThanOrEqual(surfaceBox!.x);
      expect(jumpBox!.x + jumpBox!.width).toBeLessThanOrEqual(
        surfaceBox!.x + surfaceBox!.width,
      );
      await jump.focus();
      await page.keyboard.press("Enter");
      await expect(jump).toHaveCount(0);
      await expect
        .poll(() =>
          transcript.evaluate(
            (element) =>
              element.scrollHeight - element.clientHeight - element.scrollTop,
          ),
        )
        .toBeLessThanOrEqual(1);
      await expectNoOverflow(page);
    });

    test(`${harness} home project appears once at ${width}`, async ({
      page,
    }) => {
      await openChrome(page, harness, width, "home");
      const home = page.getByRole("region", { name: "New chat", exact: true });
      const heading = home.getByRole("heading", {
        name: "What should we work on?",
        level: 3,
      });
      await expect(heading).toHaveCSS("font-weight", "400");
      await expect(home).toHaveText(
        "What should we work on?Sage project with a long name",
      );
      const project = home.getByRole("button", {
        name: "Project: Sage project with a long name",
        exact: true,
      });
      await project.focus();
      await page.keyboard.press("Enter");
      await expect(
        page.getByRole("listbox", {
          name: "Project: Sage project with a long name",
        }),
      ).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(project).toBeFocused();
      await expectNoOverflow(page);
    });
  }
}

for (const harness of ["codex", "claude-code", "pi", "opencode", "oh-my-pi"]) {
  for (const width of [1888, 720, 480]) {
    test(`${harness} compact status variants at ${width}`, async ({ page }) => {
      for (const [status, tone, harnessStatus] of [
        ["Running", "working", "Live"],
        ["Waiting for approval", "waiting", "Live"],
        ["Failed", "failed", "Live"],
        ["Queued", "working", "Sign in required"],
      ]) {
        await openChrome(
          page,
          harness,
          width,
          "transcript",
          status,
          tone,
          harnessStatus,
        );
        await expectSingleHeaderRow(page, width);
        const indicator = page.getByRole("status", {
          name: `Chat status: ${status}`,
          exact: true,
        });
        await expect(indicator).toHaveAttribute("title", status);
        await expect(
          page.getByLabel(`Harness status: ${harnessStatus}`, { exact: true }),
        ).toHaveAttribute("title", harnessStatus);
        await expect(
          indicator.locator(".agent-chat-chip__spinner"),
        ).toHaveCount(tone === "working" ? 1 : 0);
        await indicator.focus();
        await expect(indicator).toBeFocused();
        if (tone === "waiting" || tone === "failed")
          await expect(indicator.locator("svg")).toBeVisible();
        const harnessIndicator = page.getByLabel(
          `Harness status: ${harnessStatus}`,
          { exact: true },
        );
        await harnessIndicator.focus();
        await page.keyboard.press("Enter");
        await expect(
          page.locator(".agent-task-header__harness-status > p"),
        ).toHaveText(harnessStatus);
        await expectNoOverflow(page);
      }
    });
  }
  test(`${harness} multiple open chats show only the selected home`, async ({
    page,
  }) => {
    await openChrome(page, harness, 1888, "home-split");
    const homes = page.getByRole("region", { name: "New chat", exact: true });
    await expect(homes).toHaveCount(1);
    for (const home of await homes.all()) {
      await expect(
        home.getByText("Sage project with a long name", { exact: true }),
      ).toBeVisible();
    }
    await expectNoOverflow(page);
  });
}

async function expectNoOverflow(page: Page) {
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
}
