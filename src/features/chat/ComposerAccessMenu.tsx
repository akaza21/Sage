import { useEffect, useState, type RefObject } from "react";
import type {
  SessionAccessMode,
  SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { SESSION_ACCESS_MODE_SCOPE } from "../../domain/sessionAccessMode";
import { Check } from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";
import { ModalDialog } from "../../ui/ModalDialog";
import { ACCESS_ICONS } from "./composerAccessIcons";

export default function ComposerAccessMenu({
  id,
  triggerRef,
  candidates,
  effective,
  onSelect,
  onDismiss,
}: {
  id: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  candidates: readonly SessionAccessModeCapability[];
  effective: SessionAccessMode | null;
  onSelect: (mode: SessionAccessMode) => void;
  onDismiss: () => void;
}) {
  const [confirmation, setConfirmation] =
    useState<SessionAccessModeCapability | null>(null);
  const [closing, setClosing] = useState(false);
  useEffect(() => {
    if (!closing) return;
    onDismiss();
    triggerRef.current?.focus();
  }, [closing, onDismiss, triggerRef]);
  const close = () => setClosing(true);
  if (closing) return null;
  if (confirmation && !candidates.includes(confirmation)) setConfirmation(null);
  if (confirmation && candidates.includes(confirmation))
    return (
      <ModalDialog
        className="dialog--scrim"
        labelledBy={id + "-confirm"}
        onDismiss={close}
      >
        <h2 id={id + "-confirm"}>Switch to {confirmation.label}?</h2>
        <p>{confirmation.description}</p>
        <p>{SESSION_ACCESS_MODE_SCOPE}</p>
        <div className="dirty-dialog__actions">
          <button
            autoFocus
            className="button--compact"
            type="button"
            onClick={close}
          >
            Cancel
          </button>
          <button
            className="button--compact"
            type="button"
            onClick={() => {
              onSelect(confirmation.id);
              close();
            }}
          >
            Switch access
          </button>
        </div>
      </ModalDialog>
    );
  return (
    <MenuPopover
      id={id}
      open
      triggerRef={triggerRef}
      ariaLabel="Access mode"
      ariaDescribedBy={id + "-heading"}
      className="composer__popover composer-access-menu"
      initialFocus="selected"
      focusKey={
        effective + ":" + candidates.map((candidate) => candidate.id).join()
      }
      restoreFocusOnTab
      onDismiss={onDismiss}
      gap={8}
    >
      <p
        id={id + "-heading"}
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
            aria-describedby={id + "-" + candidate.id}
            tabIndex={-1}
            onClick={() => {
              if (
                candidate.id === "workspaceWrite" &&
                effective !== candidate.id
              )
                setConfirmation(candidate);
              else {
                if (effective !== candidate.id) onSelect(candidate.id);
                close();
              }
            }}
          >
            <Icon size={14} aria-hidden="true" />
            <span>{candidate.label}</span>
            {candidate.id === effective ? (
              <Check size={14} aria-hidden="true" />
            ) : null}
            <span
              id={id + "-" + candidate.id}
              className="composer-access-menu__description"
            >
              {candidate.description}
            </span>
          </button>
        );
      })}
    </MenuPopover>
  );
}
