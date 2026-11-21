import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { HarnessDefinition } from "../../domain/harness";
import { refreshHarnessSignIn, startHarnessSignIn } from "./harnessSignIn";
import { signInProvider } from "./harnessSignInProvider";

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
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const checkRequirement = useWorkbenchStore(() =>
    check ? signInProvider(harness.id)?.refreshRequirement : null,
  );
  const canCancel = useWorkbenchStore(
    (state) =>
      menuItem &&
      !check &&
      state.pendingHarnessSignIn?.bindingId === harness.bindingId,
  );
  return (
    <>
      <button
        type="button"
        role={menuItem ? "menuitem" : undefined}
        className={menuItem ? "menu-item" : "button button--compact"}
        aria-label={
          canCancel
            ? `Cancel sign-in to ${harness.name}`
            : check
              ? menuItem
                ? `Check sign-in for ${harness.name}`
                : "Check sign-in"
              : `Sign in to ${harness.name}`
        }
        disabled={!!checkRequirement || (canCancel ? cancelling : busy)}
        onClick={() => {
          setError(null);
          if (canCancel) {
            setCancelling(true);
            void useWorkbenchStore
              .getState()
              .cancelHarnessSignIn()
              .catch(() => setError("Could not cancel sign-in. Try again."))
              .finally(() => setCancelling(false));
            return;
          }
          setBusy(true);
          const action = check
            ? refreshHarnessSignIn(harness.id).then((state) => {
                if (state !== "signedIn" && state !== "signedOut")
                  throw new Error(state);
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
          {canCancel
            ? "Cancel sign-in"
            : busy
              ? check
                ? "Checking\u2026"
                : "Signing in\u2026"
              : check
                ? "Check sign-in"
                : "Sign in"}
        </span>
      </button>
      {checkRequirement ? <span>{checkRequirement}</span> : null}
      {error ? <span role="alert">{error}</span> : null}
    </>
  );
}
