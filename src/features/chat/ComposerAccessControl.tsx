import { lazy, Suspense, useId, useRef } from "react";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import {
  CHAT_ACCESS_SCOPE,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { ACCESS_ICONS } from "./composerAccessIcons";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import { sessionAccessTarget } from "./sessionAccessTarget";
const AccessMenu = lazy(() => import("./ComposerAccessMenu"));

export function ComposerAccessControl({
  bindingId,
  capabilities,
  sessionConfig,
  disabled,
  open,
  onToggle,
  onDismiss,
}: {
  bindingId: string;
  capabilities: readonly SessionAccessModeCapability[] | undefined;
  /** The live session a choice and session-mode availability apply to. */
  sessionConfig?: AcpSessionConfig | null;
  disabled: boolean;
  open: boolean;
  onToggle: () => void;
  onDismiss: () => void;
}) {
  const id = useId();
  const session = sessionAccessTarget(sessionConfig);
  const trigger = useRef<HTMLButtonElement>(null);
  const { ready, error, chatError, candidates, effective } =
    useSessionAccessSelection(bindingId, capabilities, session);
  const selected = candidates.find((candidate) => candidate.id === effective);
  // The menu carries the other modes' icons; the label names every mode.
  const Icon =
    ACCESS_ICONS[(effective ?? "workspaceWrite") as keyof typeof ACCESS_ICONS];
  if (candidates.length === 0) return null;
  const problem = error ?? chatError;
  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={`composer__control-trigger composer__access-label${effective === "workspaceWrite" ? " composer__access-label--write" : effective === "fullAccess" ? " composer__access-label--full" : ""}`}
        aria-label="Access options"
        aria-describedby={`${id}-scope`}
        aria-haspopup="menu"
        aria-controls={id}
        aria-expanded={open && !disabled && ready}
        data-selected-id={effective}
        disabled={disabled || !ready}
        onClick={onToggle}
      >
        {Icon ? <Icon size={13} aria-hidden="true" /> : null}
        <span className="composer__access-text">
          {selected?.label ?? "Choose access"}
        </span>
      </button>
      {open && !disabled && ready ? (
        <Suspense
          fallback={
            <ComposerMenuLoading triggerRef={trigger} onDismiss={onDismiss} />
          }
        >
          <AccessMenu
            key={bindingId + ":" + (session?.sessionId ?? "")}
            id={id}
            triggerRef={trigger}
            candidates={candidates}
            effective={effective}
            onDismiss={onDismiss}
            sessionId={session?.sessionId ?? null}
          />
        </Suspense>
      ) : null}
      <span id={`${id}-scope`} className="sr-only">
        {selected?.description} {CHAT_ACCESS_SCOPE}
      </span>
      {problem ? (
        <span
          className="select-control__reason composer-access-error"
          role="alert"
        >
          {problem}
        </span>
      ) : null}
    </>
  );
}
