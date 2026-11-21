import { expect, test } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
  expectNoViewportOverflow,
} from "./shared/visualBrowserPrimitives";

test.use({ viewport: { width: 960, height: 800 } });

for (const adapter of ["cli", "account"] as const) {
  test(`${adapter} sign-in problems stay in the account menu while composing`, async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    const name = adapter === "cli" ? "Claude Code" : "Codex";
    await page.evaluate(async (adapter) => {
      const { installChatChromeFixture } =
        await import("/tests/ui/fixtures/chatChromeFixture.ts");
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const { acpDesktop } = await import("/src/services/acpDesktop.ts");
      const { acpHarnessDesktop } =
        await import("/src/services/acpHarnessDesktop.ts");
      const { refreshHarnessSignIn } =
        await import("/src/features/harness/harnessSignIn.ts");
      installChatChromeFixture(
        adapter === "cli" ? "claude-code" : "codex",
        true,
      );
      let signedIn = false;
      let nativeTime = 100;
      acpDesktop.getClaudeAuthStatus = async () => ({
        loggedIn: signedIn,
        accountReadStartedAt: ++nativeTime,
      });
      acpDesktop.startClaudeAuthLogin = async () => {
        signedIn = true;
      };
      acpHarnessDesktop.startCodexAuth = async (scope) => {
        signedIn = true;
        return { ...scope, phase: "signedIn", revision: "2" };
      };
      acpHarnessDesktop.discoverCodex = async (scope) => ({
        ...scope,
        rootId: useWorkbenchStore.getState().project!.roots[0]?.id ?? null,
        account: signedIn ? "managedChatgpt" : "signedOut",
        accountReadStartedAt: ++nativeTime,
        models: [],
      });
      useWorkbenchStore.setState((state) => ({
        harnessAccountReadStartedAt: ++nativeTime,
        harnessAccount: adapter === "account" ? "signedOut" : null,
        harnessDiscoveryPhase:
          adapter === "account" ? "signInRequired" : "ready",
        harnesses: state.harnesses.map((harness) => ({
          ...harness,
          accountModelDiscovery: adapter === "account",
          signIn:
            adapter === "cli"
              ? { command: "", hint: "", statusCheck: "claudeCli" as const }
              : null,
        })),
      }));
      if (adapter === "cli")
        await refreshHarnessSignIn(
          useWorkbenchStore.getState().selectedHarnessId!,
        );
    }, adapter);
    const notices = page.getByRole("region", { name: "Composer notices" });
    await expect(notices).toHaveCount(0);
    const input = page.getByRole("combobox", { name: "Message agent" });
    await expect(input).toBeVisible();
    const harness = page.getByRole("button", { name: "Harness", exact: true });
    await harness.click();
    await expect(
      page.getByRole("listbox", { name: "Harness" }),
    ).not.toContainText(/signed in|signed out|checking sign-in/i);
    await page.keyboard.press("Escape");
    const account = page.getByRole("button", {
      name: "Account: sign-in needed",
    });
    await expect(
      account.getByRole("img", { name: "Harness sign-in needs attention" }),
    ).toHaveCSS("width", "7px");
    await account.click();
    const menu = page.getByRole("menu", { name: "Harness sign-in" });
    await expect(
      menu.getByRole("menuitem", { name: `Sign in to ${name}` }),
    ).toBeVisible();
    await expect(
      menu.getByRole("menuitem", { name: `Check sign-in for ${name}` }),
    ).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(account).toBeFocused();
    await account.click();
    await menu.getByRole("menuitem", { name: `Sign in to ${name}` }).click();
    await expect(notices.getByText(`${name} isn't signed in`)).toHaveCount(0);
    await expect(
      page.getByRole("img", { name: "Harness sign-in needs attention" }),
    ).toHaveCount(0);
    await expectNoViewportOverflow(page);
  });
}
