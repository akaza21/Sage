import { useState } from "react";
import type { HarnessDefinition } from "../../domain/harness";
import { refreshHarnessSignIn, startHarnessSignIn } from "./harnessSignIn";

export function HarnessLoginAction({
  harness,
  menuItem = false,
  check = false,
}: {
  harness: HarnessDefinition;
  menuItem?: boolean;
  check?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <>
      <button
        type="button"
        role={menuItem ? "menuitem" : undefined}
        className={menuItem ? "menu-item" : "button button--compact"}
        aria-label={check ? "Check sign-in" : `Sign in to ${harness.name}`}
        disabled={busy}
        onClick={() => {
          setBusy(true);
          setError(null);
          const action = check
            ? refreshHarnessSignIn(harness.id).then((state) => {
                if (state === "unavailable")
                  throw new Error("Check unavailable");
                if (state === "signedOut")
                  setError(`${harness.name} is still signed out.`);
              })
            : startHarnessSignIn(harness.id);
          void action
            .catch(() =>
              setError(
                `Could not ${check ? "check" : "start"} sign-in. ${harness.signIn?.command ? `Run ${harness.signIn.command} in a terminal.` : "Try again."}`,
              ),
            )
            .finally(() => setBusy(false));
        }}
      >
        {menuItem ? <span>{harness.name}</span> : null}
        <span>
          {busy
            ? check
              ? "Checking\u2026"
              : "Signing in\u2026"
            : check
              ? "Check sign-in"
              : "Sign in"}
        </span>
      </button>
      {error ? <span role="alert">{error}</span> : null}
    </>
  );
}
