import { useState, type FormEvent } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { CodexAccount } from "../../domain/acp";
import { loginCodexApiKey } from "../../services/codexApiKeyDesktop";
import { SettingsRow } from "./SettingsRow";

const ACCOUNT_TEXT: Record<CodexAccount, string> = {
  managedChatgpt: "Signed in with ChatGPT.",
  apiKey: "Signed in with an OpenAI API key.",
  providerManaged: "Credentials are managed by the configured model provider.",
  signedOut: "Signed out.",
};

/** Account controls for the selected account-discovery harness. */
export function CodexAccountSettings({ disabled }: { disabled: boolean }) {
  const account = useWorkbenchStore((state) => state.harnessAccount);
  const phase = useWorkbenchStore((state) => state.harnessDiscoveryPhase);
  const logout = useWorkbenchStore((state) => state.logoutHarness);
  const signIn = useWorkbenchStore((state) => state.signInHarnessWith);
  const [apiKey, setApiKey] = useState("");
  const busy = disabled || phase === "checking" || phase === "waiting";
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const key = apiKey.trim();
    setApiKey("");
    if (key) void signIn((scope) => loginCodexApiKey(scope, key));
  };
  return (
    <>
      <SettingsRow
        title="Account"
        description={account ? ACCOUNT_TEXT[account] : "Account not checked."}
      >
        <button
          className="button button--compact"
          type="button"
          disabled={
            busy ||
            phase !== "ready" ||
            (account !== "managedChatgpt" && account !== "apiKey")
          }
          onClick={() => void logout()}
        >
          Sign out
        </button>
      </SettingsRow>
      <SettingsRow
        title="Use an API key"
        description="Sent once to the harness, which stores it. Sage keeps no copy."
      >
        <form className="agent-settings__inline-form" onSubmit={submit}>
          <input
            className="input"
            type="password"
            aria-label="OpenAI API key"
            autoComplete="off"
            spellCheck={false}
            maxLength={4096}
            value={apiKey}
            disabled={busy}
            onChange={(event) => setApiKey(event.target.value)}
          />
          <button
            className="button button--compact"
            type="submit"
            disabled={busy || !apiKey.trim()}
          >
            Sign in
          </button>
        </form>
      </SettingsRow>
    </>
  );
}
