import { lazy, Suspense, useId, useRef } from "react";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import {
  FULL_ACCESS,
  FULL_ACCESS_SCOPE,
  SESSION_ACCESS_MODE_SCOPE,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { ACCESS_ICONS } from "./composerAccessIcons";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import { sessionAccessTarget } from "./sessionAccessTarget";
const AccessMenu = lazy(() => import("./ComposerAccessMenu"));

const LABEL_TONE: Partial<Record<string, string>> = {
  workspaceWrite: " composer__access-label--write",
  [FULL_ACCESS]: " composer__access-label--full",
};

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
  /** The live session Full access and session-mode availability apply to. */
  sessionConfig?: AcpSessionConfig | null;
  disabled: boolean;
  open: boolean;
  onToggle: () => void;
  onDismiss: () => void;
}) {
  const id = useId();
  const session = sessionAccessTarget(sessionConfig);
  const trigger = useRef<HTMLButtonElement>(null);
  const { ready, error, fullAccessError, candidates, effective } =
    useSessionAccessSelection(bindingId, capabilities, session);
  const selected = candidates.find((candidate) => candidate.id === effective);
  const Icon = ACCESS_ICONS[effective ?? "workspaceWrite"];
  if (candidates.length === 0) return null;
  const problem = error ?? fullAccessError;
  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={`composer__control-trigger composer__access-label${LABEL_TONE[effective ?? ""] ?? ""}`}
        aria-label="Access options"
        aria-describedby={`${id}-scope`}
        aria-haspopup="menu"
        aria-controls={id}
        aria-expanded={open && !disabled && ready}
        data-selected-id={effective}
        disabled={disabled || !ready}
        onClick={onToggle}
      >
        <Icon size={13} aria-hidden="true" />
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
            bindingId={bindingId}
            sessionId={session?.sessionId ?? null}
          />
        </Suspense>
      ) : null}
      <span id={`${id}-scope`} className="sr-only">
        {selected?.description}{" "}
        {effective === FULL_ACCESS
          ? FULL_ACCESS_SCOPE
          : SESSION_ACCESS_MODE_SCOPE}
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
