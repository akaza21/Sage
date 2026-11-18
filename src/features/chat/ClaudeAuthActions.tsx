import { useEffect, useRef, useState } from "react";

import type { HarnessDefinition } from "../../domain/harness";
import { acpDesktop } from "../../services/acpDesktop";

export function SignInRequiredMessage({
  harness: { name, signIn },
}: {
  harness: Pick<HarnessDefinition, "name" | "signIn">;
}) {
  if (!signIn)
    return `${name} requires sign-in before it can continue. Sign in with its CLI, then try again.`;
  const run = signIn.command ? `Run ${signIn.command}. ` : "";
  return `${name} requires sign-in before it can continue. ${run}${signIn.hint} Then try again.`;
}

export function ClaudeAuthActions({ onSignedIn }: { onSignedIn: () => void }) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  // Null once unmounted: a check that outlives this chat scope must not clear
  // the next scope's error.
  const signedIn = useRef<(() => void) | null>(null);
  useEffect(() => {
    signedIn.current = onSignedIn;
    return () => {
      signedIn.current = null;
    };
  }, [onSignedIn]);

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
        const done = signedIn.current;
        if (!done) return;
        if (status.loggedIn) done();
        else setMessage("Claude is still signed out.");
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
