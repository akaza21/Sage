import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { ComposerOptionsPart } from "./lazyParts";
import type { ReactNode } from "react";
import {
  preferredAvailableHarness,
  type HarnessDefinition,
} from "../../domain/harness";
import { useWorkbenchStore } from "../../app/store";

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
  const errors = [
    ...new Set([workbenchError, chatError, imageStatus].filter(Boolean)),
  ];
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

/** Owns unavailable-harness guidance while the composer is unmounted. */
export function HarnessUnavailableNotice({
  existingChat = false,
  workbenchError = null,
  chatError = null,
  onRetryLoad,
  onEditSummaryMount,
}: {
  onEditSummaryMount?: ChatComposerRegionProps["onEditSummaryMount"];
  existingChat?: boolean;
  workbenchError?: string | null;
  chatError?: ReactNode;
  onRetryLoad?: (() => void) | null;
}) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const openDraft = useWorkbenchStore((state) => state.openNewChatDraft);
  const available = preferredAvailableHarness(harnesses);
  return (
    <div className="chat-dock-frame">
      <ComposerStatusNotices
        workbenchError={workbenchError}
        chatError={chatError}
        imageStatus={null}
        selectedHarnessId={null}
        missingBindingNotice={false}
        discoveryPhase="ready"
      />
      {onRetryLoad ? (
        <div className="composer__error" role="alert">
          This chat failed to load.{" "}
          <button
            type="button"
            className="button button--compact"
            onClick={onRetryLoad}
          >
            Retry
          </button>
        </div>
      ) : null}
      <div className="chat-dock surface surface--dock">
        <div className="chat-edit-summary-zone" ref={onEditSummaryMount} />
        <div className="chat-harness-unavailable" role="status">
          <p>
            {existingChat
              ? "This chat used a harness that isn't available here."
              : available
                ? "The selected harness isn't available here."
                : "No harness is available here. Install or enable a harness in Settings to start a chat."}
          </p>
          {available ? (
            <button
              type="button"
              className="button button--compact"
              disabled={transitioning}
              onClick={() => void openDraft()}
            >
              Continue in a new chat
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}
