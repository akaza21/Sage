import {
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { useWorkbenchStore } from "../../app/store";
import { isTauriRuntime } from "../../ports/desktopRuntime";
import { HarnessLoginAction } from "./HarnessSignInAction";
import type { ChatComposerRegionProps } from "../chat/chatComposerRegionProps";
import { SignInRequiredMessage } from "../chat/HarnessSignInRequiredMessage";
import { lazyParts } from "../chat/lazyParts";
import type * as MenuChunk from "./HarnessSignInMenu";
import {
  ensureHarnessSignInChecked,
  harnessesNeedingSignIn,
  harnessSignInRevision,
  subscribeHarnessSignIn,
  reportHarnessSignInRequired,
  harnessSignInStatus,
  harnessSupportsSignIn,
} from "./harnessSignIn";

const MenuPart = lazyParts<typeof MenuChunk>(
  () => import("./HarnessSignInMenu"),
);

function useHarnessSignIn() {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  useSyncExternalStore(subscribeHarnessSignIn, harnessSignInRevision);
  const key = JSON.stringify(
    harnesses.slice(0, 32).map((harness) => harness.id),
  );
  useEffect(() => {
    if (!isTauriRuntime()) return;
    const check = () => ensureHarnessSignInChecked(JSON.parse(key) as string[]);
    const focus = () =>
      ensureHarnessSignInChecked(JSON.parse(key) as string[], true);
    check();
    window.addEventListener("focus", focus);
    return () => window.removeEventListener("focus", focus);
  }, [key, harnesses, projectId, epoch]);
  return harnessesNeedingSignIn();
}

export function ComposerHarnessSignInNotice({
  harnessId,
  recovery,
  refusal,
}: {
  harnessId: string | null;
  recovery?: ChatComposerRegionProps["authRecovery"];
  refusal?: ChatComposerRegionProps["signInHarness"];
}) {
  const needsSignIn = useHarnessSignIn();
  const recoveryId = recovery?.harnessId;
  const scope = recovery?.scope;
  const onSignedIn = recovery?.onSignedIn;
  const completed = useRef<string | null>(null);
  useEffect(() => {
    if (!recoveryId) return;
    completed.current = null;
    reportHarnessSignInRequired(recoveryId);
  }, [recoveryId, scope]);
  const recoveryState = recoveryId
    ? harnessSignInStatus(recoveryId).state
    : null;
  useEffect(() => {
    if (
      recoveryId &&
      recoveryState === "signedIn" &&
      harnessSignInStatus(recoveryId).state === "signedIn" &&
      scope &&
      completed.current !== scope
    ) {
      completed.current = scope;
      onSignedIn?.();
    }
  }, [onSignedIn, recoveryId, recoveryState, scope]);
  if (refusal && (!recoveryId || !harnessSupportsSignIn(recoveryId)))
    return (
      <div className="composer__error" role="alert">
        <SignInRequiredMessage harness={refusal} />
      </div>
    );
  const harness = needsSignIn.find(
    (entry) => entry.id === (recoveryId ?? harnessId),
  );
  if (!harness) return null;
  return (
    <div className="composer__notice harness-signin-notice" role="status">
      <span>{harness.name} isn't signed in</span>
      <HarnessLoginAction key={harness.id} harness={harness} />
      {recoveryId === harness.id ? (
        <HarnessLoginAction
          key={`${harness.id}:check:${scope}`}
          harness={harness}
          check
        />
      ) : null}
    </div>
  );
}

export function HarnessAccountMenu({ avatar }: { avatar: string }) {
  const needsSignIn = useHarnessSignIn();
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  if (!needsSignIn.length) {
    if (open) setOpen(false);
    return (
      <span className="account-avatar" aria-hidden="true">
        {avatar}
      </span>
    );
  }
  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className="account-avatar harness-account"
        aria-label="Account: sign-in needed"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((value) => !value)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <span aria-hidden="true">{avatar}</span>
        <span
          className="harness-account__attention"
          role="img"
          aria-label="Harness sign-in needs attention"
        />
      </button>
      {open ? (
        <MenuPart
          part="HarnessSignInMenu"
          menuId={menuId}
          triggerRef={triggerRef}
          harnesses={needsSignIn}
          onDismiss={() => setOpen(false)}
        />
      ) : null}
    </>
  );
}
