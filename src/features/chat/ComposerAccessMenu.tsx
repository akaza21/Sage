import { useEffect, useState, type RefObject } from "react";
import {
  chooseChatAccess,
  grantChatAccess,
  PERMISSIVE_ACCESS as PERMISSIVE,
  prepareChatAccess,
} from "../../app/sessionAccessActions";
import type {
  SessionAccessMode,
  SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { SESSION_ACCESS_MODE_SCOPE } from "../../domain/sessionAccessMode";
import { Check } from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";
import { ModalDialog } from "../../ui/ModalDialog";
import { CHAT_ACCESS_ICONS as ICONS } from "./chatAccessIcons";

const FULL_ACCESS: SessionAccessMode = "fullAccess";

const CONFIRMED: readonly SessionAccessMode[] = [
  "workspaceWrite",
  ...PERMISSIVE,
];

const WARNING: Partial<Record<SessionAccessMode, string>> = {
  auto: "Claude's own checks decide which actions run without asking. Sage does not review them, and there is no filesystem sandbox.",
  fullAccess:
    "The agent can change or delete files and run commands without asking, with no filesystem sandbox.",
};

export default function ComposerAccessMenu({
  id,
  triggerRef,
  candidates,
  effective,
  sessionId,
  onDraft,
  onDismiss,
}: {
  id: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  candidates: readonly SessionAccessModeCapability[];
  effective: SessionAccessMode | null;
  sessionId?: string;
  onDraft?: (mode: SessionAccessMode) => void;
  onDismiss: () => void;
}) {
  const [confirmation, setConfirmation] =
    useState<SessionAccessModeCapability | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [closing, setClosing] = useState(false);
  const permissive =
    confirmation !== null && PERMISSIVE.includes(confirmation.id)
      ? confirmation.id
      : null;
  useEffect(() => {
    if (!closing) return;
    onDismiss();
    triggerRef.current?.focus();
  }, [closing, onDismiss, triggerRef]);
  // The native side issues a single-use token, bound to this chat and mode,
  // when the dialog opens; only the confirm below spends it, so a stale or
  // replayed command cannot skip the confirmation.
  useEffect(() => {
    if (!permissive || !sessionId) return;
    let live = true;
    void prepareChatAccess(sessionId, permissive).then((issued) => {
      if (!live) return;
      if (issued) setToken(issued);
      else setClosing(true);
    });
    return () => {
      live = false;
    };
  }, [permissive, sessionId]);
  const close = () => setClosing(true);
  const choose = (mode: SessionAccessMode) => {
    if (sessionId) void chooseChatAccess(sessionId, mode);
    else onDraft?.(mode);
  };
  const selectable = !!sessionId || !!onDraft;
  if (closing) return null;
  if (confirmation && !candidates.includes(confirmation)) setConfirmation(null);
  if (confirmation && selectable && candidates.includes(confirmation)) {
    const warning = WARNING[confirmation.id];
    return (
      <ModalDialog
        className="dialog--scrim"
        labelledBy={id + "-confirm"}
        onDismiss={close}
      >
        <h2 id={id + "-confirm"}>
          {permissive
            ? `Allow ${confirmation.label.toLowerCase()} in this chat?`
            : `Switch to ${confirmation.label}?`}
        </h2>
        <p>{confirmation.description}</p>
        {warning ? (
          <p className="composer-access-menu__warning">
            {warning} Other chats and new chats keep their own access.
          </p>
        ) : null}
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
            disabled={permissive !== null && !!sessionId && token === null}
            onClick={() => {
              // A drafted permissive mode is confirmed natively once its
              // session has started, before the first prompt.
              if (!permissive || !sessionId) choose(confirmation.id);
              else if (token)
                void grantChatAccess(sessionId, permissive, token);
              close();
            }}
          >
            {permissive
              ? `Allow ${confirmation.label.toLowerCase()}`
              : "Switch access"}
          </button>
        </div>
      </ModalDialog>
    );
  }
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
        {sessionId || onDraft
          ? "How should agent actions be approved in this chat?"
          : "This chat's access can change once its session starts. Defaults for new chats are in Settings."}
      </p>
      {candidates.map((candidate) => {
        const Icon = ICONS[candidate.id];
        return (
          <button
            key={candidate.id}
            type="button"
            role="menuitemradio"
            className={`menu-item composer-access-menu__option${candidate.id === FULL_ACCESS ? " composer-access-menu__option--full" : ""}`}
            aria-label={candidate.label}
            aria-checked={candidate.id === effective}
            aria-disabled={!selectable}
            data-option-id={candidate.id}
            aria-describedby={id + "-" + candidate.id}
            tabIndex={-1}
            onClick={() => {
              if (!selectable) return;
              if (effective === candidate.id) close();
              else if (CONFIRMED.includes(candidate.id))
                setConfirmation(candidate);
              else {
                choose(candidate.id);
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
