import { useState, useSyncExternalStore } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { HarnessDefinition } from "../../domain/harness";
import {
  harnessSignInChecking,
  refreshHarnessSignIn,
  startHarnessSignIn,
  subscribeHarnessSignInChecking,
} from "./harnessSignIn";
import { signInProvider } from "./harnessSignInProvider";

/**
 * A menu item is the harness's one sign-in row: Sage rechecks the account on
 * window focus and after a sign-in, so the row shows that check inline
 * instead of offering a second Check row. `check` is the chat recovery's
 * explicit read after a refused turn.
 */
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
      menuItem && state.pendingHarnessSignIn?.bindingId === harness.bindingId,
  );
  const checking = useSyncExternalStore(
    subscribeHarnessSignInChecking,
    () => menuItem && harnessSignInChecking(harness.id),
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
              ? "Check sign-in"
              : `Sign in to ${harness.name}`
        }
        disabled={
          !!checkRequirement || (canCancel ? cancelling : busy || checking)
        }
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
            : busy && !check
              ? "Signing in…"
              : busy || checking
                ? "Checking…"
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
