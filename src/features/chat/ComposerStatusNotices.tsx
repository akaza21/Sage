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

/** Errors remain available while the optional notice surface loads. */
export function ComposerNoticeFrame(
  props: ComposerStatusNoticeProps & { children?: ReactNode },
) {
  const { workbenchError, chatError, imageStatus, children } = props;
  const errors = [workbenchError, chatError, imageStatus].filter(Boolean);
  const missingBinding =
    props.missingBindingNotice &&
    (props.discoveryPhase === "idle" || props.discoveryPhase === "ready");
  return errors.length || missingBinding || children ? (
    <div
      className="composer__notices"
      role="region"
      aria-label="Composer notices"
      tabIndex={0}
    >
      {children}
      {errors.map((error, index) => (
        <div key={index} className="composer__error" role="alert">
          {error}
        </div>
      ))}
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
    props.authRecovery || props.chatAuth?.errorKind === "signInRequired";
  return refusal ? (
    <ComposerOptionsPart
      part="ComposerNoticeSurface"
      {...props}
      fallback={<ComposerNoticeFrame {...props} />}
    />
  ) : (
    <ComposerNoticeFrame {...props} />
  );
}
