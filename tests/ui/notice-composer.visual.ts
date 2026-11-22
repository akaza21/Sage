import { expect, test, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

async function noticeScenario(
  page: Page,
  state: "checking" | "consent" | "binding" | "build",
) {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async (state) => {
    const { installChatChromeFixture } =
      await import("/tests/ui/fixtures/chatChromeFixture.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    installChatChromeFixture("codex", false, "completed");
    const harness = useWorkbenchStore.getState().harnesses[0];
    if (state === "checking" || state === "consent")
      useWorkbenchStore.setState({
        harnesses: [{ ...harness, accountModelDiscovery: true }],
        harnessDiscoveryPhase: state,
      });
    if (state === "binding")
      useWorkbenchStore.setState({
        harnesses: [
          {
            ...harness,
            installState: "unavailable",
            bindingId: null,
            unavailableReason: "adapterAbsent",
          },
        ],
      });
    if (state === "build")
      useWorkbenchStore.setState({
        harnesses: [
          {
            ...harness,
            boundVersion: "1.2.3",
            characterisation: "uncharacterised",
            update: {
              latestVersion: "1.3.0",
              command: "npm install -g example-agent@latest",
            },
          },
        ],
      });
  }, state);
}

test("checking stays in the harness picker with no chat warning", async ({
  page,
}) => {
  await noticeScenario(page, "checking");
  await expect(
    page.getByRole("combobox", { name: "Message agent" }),
  ).toBeEnabled();
  await expect(
    page.locator(
      ".chat-surface > .composer__notice, .chat-surface .notice--warning",
    ),
  ).toHaveCount(0);
  await expect(page.getByText(/Checking the Codex account/)).toHaveCount(0);
  await page.getByRole("button", { name: "Harness", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Harness", exact: true });
  await expect(dialog.getByRole("status")).toHaveText(
    /Checking the Codex account/,
  );
  await expect(dialog).toHaveScreenshot("composer-notice-checking.png");
});

test("consent is local, keyboard reachable, and unmounts on close", async ({
  page,
}) => {
  await noticeScenario(page, "consent");
  await expect(page.getByText(/stores credentials/)).toHaveCount(0);
  const trigger = page.getByRole("button", { name: "Harness", exact: true });
  await trigger.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Harness", exact: true });
  await expect(dialog.getByText(/stores credentials/)).toBeVisible();
  await expect(trigger).toHaveAttribute(
    "aria-controls",
    (await dialog.getAttribute("id"))!,
  );
  await expect(
    page.getByRole("combobox", { name: "Message agent" }),
  ).toBeEnabled();
  // Cold loading may leave focus on the option; reverse Tab reaches actions.
  const cancel = dialog.getByRole("button", { name: "Cancel", exact: true });
  const proceed = dialog.getByRole("button", { name: "Continue", exact: true });
  if (
    await dialog
      .getByRole("option")
      .evaluate((node) => node === document.activeElement)
  ) {
    await page.keyboard.press("Shift+Tab");
    await expect(cancel).toBeFocused();
    await page.keyboard.press("Shift+Tab");
  }
  await expect(proceed).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(cancel).toBeFocused();
  await expect(dialog).toHaveScreenshot("composer-notice-consent.png");
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await expect(page.getByText(/stores credentials/)).toHaveCount(0);
});

test("a cold missing binding has one explanation and keyboard recovery", async ({
  page,
}) => {
  await noticeScenario(page, "binding");
  let release!: () => void;
  const loaded = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route(
    "**/src/features/chat/HarnessDiscoveryNotice.tsx*",
    async (route) => {
      await loaded;
      await route.continue();
    },
  );
  try {
    const send = page.getByRole("button", {
      name: "Send message",
      exact: true,
    });
    await expect(send).toBeDisabled();
    await expect(send).toHaveAccessibleDescription(
      "The selected harness is unavailable.",
    );
    const trigger = page.getByRole("button", { name: "Harness", exact: true });
    await trigger.focus();
    await page.keyboard.press("Enter");
    const dialog = page.getByRole("dialog", { name: "Harness", exact: true });
    await expect(dialog).toBeFocused();
    release();
    await expect(dialog.getByRole("alert")).toHaveText(
      /ACP adapter isn't installed/,
    );
    await expect(
      page.getByText("This harness's ACP adapter isn't installed.", {
        exact: true,
      }),
    ).toHaveCount(1);
    await expect(dialog).toHaveScreenshot("composer-notice-binding.png");
    await page.keyboard.press("Tab");
    const recovery = dialog.getByRole("button", {
      name: "Open Settings",
      exact: true,
    });
    await expect(recovery).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(
      page.getByRole("button", { name: "Refresh discovery", exact: true }),
    ).toBeVisible();
  } finally {
    release();
  }
});

test("compatibility and update have separate severities and expandable installation detail", async ({
  page,
}) => {
  await noticeScenario(page, "build");
  await page.getByRole("button", { name: "Harness", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Harness", exact: true });
  await expect(dialog.locator(".notice--warning")).toHaveText(
    /some features are off/,
  );
  await expect(dialog.locator(".notice--info")).toHaveText(
    /Codex 1.3.0 is available/,
  );
  await expect(
    dialog.getByText("npm install -g example-agent@latest", { exact: true }),
  ).toHaveCount(0);
  await expect(dialog).toHaveScreenshot("composer-notice-build.png");
  await dialog.getByRole("button", { name: "Details", exact: true }).click();
  await expect(
    dialog.getByText("npm install -g example-agent@latest", { exact: true }),
  ).toBeVisible();
});
