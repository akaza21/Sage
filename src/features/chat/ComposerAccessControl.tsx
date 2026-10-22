import { useId, useRef } from "react";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import {
  SESSION_ACCESS_MODE_SCOPE,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import {
  Check,
  Eye,
  MessageCircleQuestion,
  ShieldCheck,
} from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";

const ACCESS_ICONS = {
  readOnly: Eye,
  ask: MessageCircleQuestion,
  workspaceWrite: ShieldCheck,
};

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
        <ShieldCheck size={13} aria-hidden="true" />
        <span className="composer__access-text">
          {selected?.label ?? "Choose access"}
        </span>
      </button>
      <MenuPopover
        id={id}
        open={open && !disabled && ready}
        triggerRef={trigger}
        ariaLabel="Access mode"
        ariaDescribedBy={`${id}-heading`}
        className="composer__popover composer-access-menu"
        initialFocus="selected"
        focusKey={`${effective}:${candidates.map((candidate) => candidate.id).join()}`}
        restoreFocusOnTab
        onDismiss={onDismiss}
        gap={8}
      >
        <p
          id={`${id}-heading`}
          className="composer-access-menu__heading"
          aria-hidden="true"
        >
          How should agent actions be approved?
        </p>
        {candidates.map((candidate) => {
          const Icon = ACCESS_ICONS[candidate.id];
          return (
            <button
              key={candidate.id}
              type="button"
              role="menuitemradio"
              className="menu-item composer-access-menu__option"
              aria-label={candidate.label}
              aria-checked={candidate.id === effective}
              data-option-id={candidate.id}
              aria-describedby={`${id}-${candidate.id}`}
              tabIndex={-1}
              onClick={() => {
                setMode(bindingId, candidate.id);
                onDismiss();
                trigger.current?.focus();
              }}
            >
              <Icon size={14} aria-hidden="true" />
              <span>{candidate.label}</span>
              {candidate.id === effective ? (
                <Check size={14} aria-hidden="true" />
              ) : null}
              <span
                id={`${id}-${candidate.id}`}
                className="composer-access-menu__description"
              >
                {candidate.description}
              </span>
            </button>
          );
        })}
      </MenuPopover>
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
