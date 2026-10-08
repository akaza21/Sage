import { useState } from "react";

import { acpDesktop } from "../../services/acpDesktop";

export function ClaudeAuthActions() {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function run(action: "login" | "status") {
    if (busy) return;
    setBusy(true);
    setMessage(null);
    try {
      if (action === "login") {
        await acpDesktop.startClaudeAuthLogin();
        setMessage("Finish in the terminal, then check here.");
      } else {
        const status = await acpDesktop.getClaudeAuthStatus();
        setMessage(
          status.loggedIn
            ? "Claude reports signed in. Send your prompt again."
            : "Claude is still signed out.",
        );
      }
    } catch {
      setMessage(
        "Could not start or check sign-in. Run claude auth login in a terminal.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <button
        className="button button--compact"
        type="button"
        disabled={busy}
        onClick={() => void run("login")}
      >
        Open Claude sign-in
      </button>
      <button
        className="button button--compact"
        type="button"
        disabled={busy}
        onClick={() => void run("status")}
      >
        Check sign-in
      </button>
      {message ? <span role="status">{message}</span> : null}
    </>
  );
}
