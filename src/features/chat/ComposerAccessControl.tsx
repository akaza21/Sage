import { lazy, Suspense, useId, useRef } from "react";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import {
  SESSION_ACCESS_MODE_SCOPE,
  type SessionAccessMode,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { ACCESS_ICONS } from "./composerAccessIcons";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
const AccessMenu = lazy(() => import("./ComposerAccessMenu"));
// A live chat's own access, its modes and their icons load off the chat path.
const ChatAccess = lazy(() => import("./ChatAccessSelection"));

/** What the access trigger shows, for the chat's defaults or its own mode. */
export interface AccessView {
  candidates: readonly SessionAccessModeCapability[];
  effective: SessionAccessMode | null;
  error: string | null;
  sessionId?: string;
  icon?: (typeof ACCESS_ICONS)["ask"];
  tone?: string;
}

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
  /** The live chat a choice applies to; without one nothing can change. */
  sessionConfig?: AcpSessionConfig | null;
  disabled: boolean;
  open: boolean;
  onToggle: () => void;
  onDismiss: () => void;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const base = useSessionAccessSelection(bindingId, capabilities);
  const ready = base.ready;
  const view = ({
    candidates,
    effective,
    error,
    sessionId,
    icon,
    tone,
  }: AccessView) => {
    const selected = candidates.find((candidate) => candidate.id === effective);
    const Icon =
      icon ??
      ACCESS_ICONS[
        (effective ?? "workspaceWrite") as keyof typeof ACCESS_ICONS
      ];
    if (candidates.length === 0) return null;
    return (
      <>
        <button
          ref={trigger}
          type="button"
          className={`composer__control-trigger composer__access-label${tone ?? (effective === "workspaceWrite" ? " composer__access-label--write" : "")}`}
          aria-label="Access options"
          aria-describedby={`${id}-scope`}
          aria-haspopup="menu"
          aria-controls={id}
          aria-expanded={open && !disabled && ready}
          data-selected-id={effective}
          disabled={disabled || !ready}
          onClick={onToggle}
        >
          {Icon && <Icon size={13} aria-hidden="true" />}
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
              key={bindingId + sessionId}
              id={id}
              triggerRef={trigger}
              candidates={candidates}
              effective={effective}
              onDismiss={onDismiss}
              sessionId={sessionId}
            />
          </Suspense>
        ) : null}
        <span id={`${id}-scope`} className="sr-only">
          {selected?.description} {SESSION_ACCESS_MODE_SCOPE}
        </span>
        {error ? (
          <span
            className="select-control__reason composer-access-error"
            role="alert"
          >
            {error}
          </span>
        ) : null}
      </>
    );
  };
  return sessionConfig && !sessionConfig.removed ? (
    <Suspense fallback={view(base)}>
      <ChatAccess
        bindingId={bindingId}
        capabilities={capabilities}
        session={sessionConfig}
        error={base.error}
      >
        {view}
      </ChatAccess>
    </Suspense>
  ) : (
    view(base)
  );
}
