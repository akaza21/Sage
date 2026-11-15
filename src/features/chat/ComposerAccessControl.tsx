import { lazy, Suspense, useId, useRef, useState } from "react";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import {
  SESSION_ACCESS_MODE_SCOPE,
  type SessionAccessMode,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { ACCESS_ICONS } from "./composerAccessIcons";
import type { EndedSession } from "./endedSession";
// A chat's own access (live or drafted), its menu, errors, modes and icons
// load off the chat path. The chunk reports what the trigger shows; until
// then it shows the default, and the same trigger stays mounted throughout.
const ChatAccess = lazy(() => import("./ChatAccessSelection"));

/** What the access trigger and menu show, for the chat's defaults or its own mode. */
export interface AccessView {
  candidates: readonly SessionAccessModeCapability[];
  effective: SessionAccessMode | null;
  error?: string | null;
  sessionId?: string;
  /** Holds a choice for a chat whose session has not started. */
  onDraft?: (mode: SessionAccessMode) => void;
  icon?: (typeof ACCESS_ICONS)["ask"];
  tone?: string;
}

export interface ComposerAccessControlProps {
  bindingId: string;
  /** The chat whose access is drafted; given only while its next send starts a session. */
  chatId?: string;
  /** The session whose access the session the next send starts takes. */
  endedSession?: EndedSession;
  capabilities: readonly SessionAccessModeCapability[] | undefined;
  /** The live chat a choice applies to; without one nothing can change. */
  sessionConfig?: AcpSessionConfig | null;
  disabled: boolean;
  open: boolean;
  onToggle: () => void;
  onDismiss: () => void;
}

export function ComposerAccessControl(props: ComposerAccessControlProps) {
  const { bindingId, capabilities, disabled, open, onToggle } = props;
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const base = useSessionAccessSelection(bindingId, capabilities);
  const [chat, setChat] = useState<AccessView | null>(null);
  const ready = base.ready;
  const shown = open && !disabled && ready;
  const { candidates, effective, icon, tone }: AccessView = chat ?? base;
  const selected = candidates.find((candidate) => candidate.id === effective);
  const Icon =
    icon ??
    ACCESS_ICONS[(effective ?? "workspaceWrite") as keyof typeof ACCESS_ICONS];
  return (
    <>
      {candidates.length > 0 && (
        <>
          <button
            ref={trigger}
            type="button"
            className={`composer__control-trigger composer__access-label${tone ?? (effective === "workspaceWrite" ? " composer__access-label--write" : "")}`}
            aria-label="Access options"
            aria-describedby={`${id}-scope`}
            aria-haspopup="menu"
            aria-controls={id}
            aria-expanded={!!chat && shown}
            data-selected-id={effective}
            disabled={disabled || !ready}
            onClick={onToggle}
          >
            {Icon && <Icon size={13} aria-hidden="true" />}
            <span className="composer__access-text">
              {selected?.label ?? "Choose access"}
            </span>
          </button>
          <span id={`${id}-scope`} className="sr-only">
            {selected?.description} {SESSION_ACCESS_MODE_SCOPE}
          </span>
        </>
      )}
      <Suspense fallback={null}>
        <ChatAccess
          {...props}
          id={id}
          triggerRef={trigger}
          open={shown}
          onView={setChat}
        />
      </Suspense>
    </>
  );
}
