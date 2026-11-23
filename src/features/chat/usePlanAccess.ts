import { useState } from "react";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import type { HarnessDefinition } from "../../domain/harness";
import {
  governedPlanMode,
  type SessionAccessMode,
} from "../../domain/sessionAccessMode";
import type { PlanAccessTarget } from "./ComposerPlanAccess";
import type { ControlledDraftPickers } from "./chatHarnessControlsProps";
import type { EndedPage } from "./endedSession";

/**
 * The "+" menu's Plan mode where access modes carry session modes, and the
 * confirmation it opens the access menu on. The confirmation is held by
 * this composer and lasts only while its access menu stays open on the same
 * binding, chat and session, so a dismissal or another target drops it.
 */
export function usePlanAccess({
  harness,
  available,
  draftChatId,
  draftPickers,
  endedPage,
  sessionConfig,
  accessOpen,
  openAccess,
}: {
  harness: HarnessDefinition | undefined;
  available: boolean;
  draftChatId?: string;
  /** The new-chat draft, whose own access the "+" menu's Plan mode drives. */
  draftPickers?: ControlledDraftPickers;
  endedPage?: EndedPage;
  sessionConfig: AcpSessionConfig | null;
  accessOpen: boolean;
  openAccess: () => void;
}) {
  // A binding id and Sage ids never contain a comma.
  const target = String([
    harness?.bindingId,
    draftChatId,
    sessionConfig?.sageAgentSessionId,
  ]);
  const [held, setHeld] = useState<{
    target: string;
    mode: SessionAccessMode;
  } | null>(null);
  const kept = held && accessOpen && held.target === target ? held : null;
  if (held && !kept) setHeld(null);
  const planAccess: PlanAccessTarget | undefined =
    available &&
    harness?.bindingId &&
    governedPlanMode(harness.accessModes) &&
    (draftChatId || draftPickers || (sessionConfig && !sessionConfig.removed))
      ? {
          bindingId: harness.bindingId,
          chatId: draftChatId,
          endedPage,
          sessionConfig,
          capabilities: harness.accessModes,
          onOpenAccess: (mode) => {
            if (mode) setHeld({ target, mode });
            openAccess();
          },
        }
      : undefined;
  return { planAccess, confirmation: kept?.mode ?? null };
}
