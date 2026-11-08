import { useEffect, useState, type RefObject } from "react";
import type {
  SessionAccessMode,
  SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import {
  FULL_ACCESS,
  FULL_ACCESS_SCOPE,
  SESSION_ACCESS_MODE_SCOPE,
} from "../../domain/sessionAccessMode";
import { Check } from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";
import { ModalDialog } from "../../ui/ModalDialog";
import { ACCESS_ICONS } from "./composerAccessIcons";

/** Modes that widen what runs without asking are confirmed first. */
const CONFIRMED: readonly SessionAccessMode[] = ["workspaceWrite", FULL_ACCESS];

export default function ComposerAccessMenu({
  id,
  triggerRef,
  candidates,
  effective,
  onSelect,
  onPrepareFullAccess,
  onDismiss,
}: {
  id: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  candidates: readonly SessionAccessModeCapability[];
  effective: SessionAccessMode | null;
  /** `confirmToken` accompanies Full access, from `onPrepareFullAccess`. */
  onSelect: (mode: SessionAccessMode, confirmToken?: string) => void;
  onPrepareFullAccess: () => Promise<string | null>;
  onDismiss: () => void;
}) {
  const [confirmation, setConfirmation] =
    useState<SessionAccessModeCapability | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [closing, setClosing] = useState(false);
  const fullAccess = confirmation?.id === FULL_ACCESS;
  useEffect(() => {
    if (!closing) return;
    onDismiss();
    triggerRef.current?.focus();
  }, [closing, onDismiss, triggerRef]);
  // The native side issues a single-use token when this dialog opens; only
  // the confirm below spends it, so a stale or replayed grant fails.
  useEffect(() => {
    if (!fullAccess) return;
    let live = true;
    void onPrepareFullAccess().then((issued) => {
      if (!live) return;
      if (issued) setToken(issued);
      else setClosing(true);
    });
    return () => {
      live = false;
    };
  }, [fullAccess, onPrepareFullAccess]);
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
        <h2 id={id + "-confirm"}>
          {fullAccess
            ? "Allow full access in this chat?"
            : `Switch to ${confirmation.label}?`}
        </h2>
        <p>{confirmation.description}</p>
        {fullAccess ? (
          <p className="composer-access-menu__warning">
            The agent can change or delete files and run commands without
            asking. Other chats and new chats keep their own access.
          </p>
        ) : null}
        <p>{fullAccess ? FULL_ACCESS_SCOPE : SESSION_ACCESS_MODE_SCOPE}</p>
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
            disabled={fullAccess && token === null}
            onClick={() => {
              onSelect(confirmation.id, fullAccess ? (token ?? "") : undefined);
              close();
            }}
          >
            {fullAccess ? "Allow full access" : "Switch access"}
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
            className={`menu-item composer-access-menu__option${candidate.id === FULL_ACCESS ? " composer-access-menu__option--full" : ""}`}
            aria-label={candidate.label}
            aria-checked={candidate.id === effective}
            data-option-id={candidate.id}
            aria-describedby={id + "-" + candidate.id}
            tabIndex={-1}
            onClick={() => {
              if (
                CONFIRMED.includes(candidate.id) &&
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
