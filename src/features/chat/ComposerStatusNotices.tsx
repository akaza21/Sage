import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { ComposerOptionsPart } from "./lazyParts";
import type { ReactNode } from "react";
import type { HarnessDefinition } from "../../domain/harness";

export type ComposerStatusNoticeProps = Pick<
  ChatComposerRegionProps,
  | "workbenchError"
  | "chatError"
  | "chatAuth"
  | "missingBindingNotice"
  | "discoveryPhase"
  | "selectedHarnessId"
> & {
  imageStatus: string | null;
  signInHarness?: Pick<HarnessDefinition, "name" | "signIn"> | null;
  authRecovery?: {
    harnessId: string;
    scope: string;
    receivedAt?: number | null;
    onSignedIn: () => void;
  };
};

export function unexplainedMissingBinding(
  props: Pick<
    ComposerStatusNoticeProps,
    "missingBindingNotice" | "discoveryPhase"
  >,
) {
  return (
    props.missingBindingNotice &&
    (props.discoveryPhase === "idle" || props.discoveryPhase === "ready")
  );
}

/** Errors remain available while the optional notice surface loads. */
export function ComposerNoticeFrame(
  props: ComposerStatusNoticeProps & { children?: ReactNode },
) {
  const { workbenchError, chatError, imageStatus, children } = props;
  const errors = [workbenchError, chatError, imageStatus];
  const missingBinding = unexplainedMissingBinding(props);
  return errors.some(Boolean) || missingBinding || children ? (
    <div
      className="composer__notices"
      role="region"
      aria-label="Composer notices"
      tabIndex={0}
    >
      {children}
      {errors.map((error, index) =>
        error ? (
          <div key={index} className="composer__error" role="alert">
            {error}
          </div>
        ) : null,
      )}
      {missingBinding ? (
        <div className="composer__notice">
          No reviewed local binding is available for this harness.
        </div>
      ) : null}
    </div>
  ) : null;
}

export function ComposerStatusNotices(props: ComposerStatusNoticeProps) {
  const refusal =
    props.signInHarness ||
    props.authRecovery ||
    props.chatAuth?.errorKind === "signInRequired";
  if (!refusal) return <ComposerNoticeFrame {...props} />;
  return (
    <ComposerOptionsPart
      part="ComposerNoticeSurface"
      {...props}
      fallback={<ComposerNoticeFrame {...props} />}
    />
  );
}
