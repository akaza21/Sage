import { lazy, Suspense, useId, useRef, useState } from "react";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import {
  SESSION_ACCESS_MODE_SCOPE,
  type SessionAccessMode,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { ACCESS_ICONS } from "./composerAccessIcons";
const ChatAccess = lazy(() => import("./ChatAccessSelection"));

export interface AccessView {
  candidates: readonly SessionAccessModeCapability[];
  effective: SessionAccessMode | null;
  error?: string | null;
  sessionId?: string;
  onDraft?: (mode: SessionAccessMode) => void;
  icon?: (typeof ACCESS_ICONS)["ask"];
  tone?: string;
}

export interface ComposerAccessControlProps {
  bindingId: string;
  chatId?: string;
  capabilities: readonly SessionAccessModeCapability[] | undefined;
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
