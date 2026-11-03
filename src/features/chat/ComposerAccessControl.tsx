import { lazy, Suspense, useId, useRef } from "react";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import {
  SESSION_ACCESS_MODE_SCOPE,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { ACCESS_ICONS } from "./composerAccessIcons";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
const AccessMenu = lazy(() => import("./ComposerAccessMenu"));

export function ComposerAccessControl({
  bindingId,
  capabilities,
  disabled,
  open,
  onToggle,
  onDismiss,
}: {
  bindingId: string;
  capabilities: readonly SessionAccessModeCapability[] | undefined;
  disabled: boolean;
  open: boolean;
  onToggle: () => void;
  onDismiss: () => void;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const { ready, error, setMode, candidates, effective } =
    useSessionAccessSelection(bindingId, capabilities);
  const selected = candidates.find((candidate) => candidate.id === effective);
  const Icon = ACCESS_ICONS[effective ?? "workspaceWrite"];
  if (candidates.length === 0) return null;
  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={`composer__control-trigger composer__access-label${effective === "workspaceWrite" ? " composer__access-label--write" : ""}`}
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
            key={bindingId}
            id={id}
            triggerRef={trigger}
            candidates={candidates}
            effective={effective}
            onDismiss={onDismiss}
            onSelect={(mode) => setMode(bindingId, mode)}
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
}
