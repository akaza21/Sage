import {
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { useWorkbenchStore } from "../../app/store";
import { isTauriRuntime } from "../../ports/desktopRuntime";
import { HarnessLoginAction } from "./HarnessSignInAction";
import type { ComposerStatusNoticeProps } from "../chat/ComposerStatusNotices";
import { lazyParts } from "../chat/lazyParts";
import type * as MenuChunk from "./HarnessSignInMenu";
import {
  ensureHarnessSignInChecked,
  harnessesNeedingSignIn,
  harnessSignInRevision,
  subscribeHarnessSignIn,
  harnessSignInStatus,
} from "./harnessSignIn";

export { HarnessTurnSignInRecovery } from "./HarnessTurnSignInRecovery";

const MenuPart = lazyParts<typeof MenuChunk>(
  () => import("./HarnessSignInMenu"),
);

function useHarnessSignIn() {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  useSyncExternalStore(subscribeHarnessSignIn, harnessSignInRevision);
  useEffect(() => {
    if (!isTauriRuntime()) return;
    const ids = harnesses.slice(0, 32).map((harness) => harness.id);
    const focus = () => ensureHarnessSignInChecked(ids, true);
    ensureHarnessSignInChecked(ids);
    window.addEventListener("focus", focus);
    return () => window.removeEventListener("focus", focus);
  }, [harnesses, projectId, epoch]);
  return harnesses;
}

export function HarnessSignInRecovery({
  recovery,
  refusal,
}: {
  recovery: NonNullable<ComposerStatusNoticeProps["authRecovery"]>;
  refusal?: ReactNode;
}) {
  const harnesses = useHarnessSignIn();
  const { harnessId: recoveryId, scope, onSignedIn, receivedAt } = recovery;
  const completed = useRef<string | null>(null);
  const { state: recoveryState, evidenceAt: accountReadStartedAt } =
    harnessSignInStatus(recoveryId);
  const awaitingFreshRead =
    receivedAt != null &&
    (accountReadStartedAt == null || accountReadStartedAt <= receivedAt);
  useEffect(() => {
    if (
      awaitingFreshRead ||
      harnessSignInStatus(recoveryId).state !== "signedIn"
    ) {
      completed.current = null;
    } else if (completed.current !== scope) {
      completed.current = scope;
      onSignedIn();
    }
  }, [onSignedIn, recoveryId, recoveryState, scope, awaitingFreshRead]);
  if (recoveryState === "unsupported") return refusal;
  const harness =
    recoveryState !== "signedIn" || awaitingFreshRead
      ? harnesses.find((entry) => entry.id === recoveryId)
      : undefined;
  if (!harness) return null;
  return (
    <div className="acp-turn__sign-in" role="status">
      <span>
        {recoveryState !== "signedOut"
          ? `Recheck ${harness.name} sign-in`
          : `${harness.name} isn't signed in`}
      </span>
      <HarnessLoginAction key={harness.id} harness={harness} />
      <HarnessLoginAction
        key={`${harness.id}:${scope}`}
        harness={harness}
        check
      />
    </div>
  );
}

export function HarnessAccountMenu({ avatar }: { avatar: string }) {
  useHarnessSignIn();
  const needsSignIn = harnessesNeedingSignIn();
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
          onDismiss={() => setOpen(false)}
        >
          {needsSignIn.map((harness) => (
            <div key={harness.id} role="none">
              <HarnessLoginAction harness={harness} menuItem />
              <HarnessLoginAction harness={harness} menuItem check />
            </div>
          ))}
        </MenuPart>
      ) : null}
    </>
  );
}
