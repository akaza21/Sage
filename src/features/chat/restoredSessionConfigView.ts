import type { AcpChatPage } from "../../domain/acp";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";

/** Said of a restored value: the session has not reapplied it yet. */
export const RESTORED_NOTE =
  "Restored from this chat's last session; applied when the chat resumes.";
/** Said of a stored value while a live session has not reported its own. */
export const PENDING_NOTE =
  "Saved for this chat; applied once the session reports its settings.";

/**
 * What the composer shows for a chat whose current session has no live
 * config yet: the config it last reported with its stored choices, marked
 * restored. It needs the session to be the one the composer would continue,
 * the same harness binding and requested model, and gives way to the live
 * config as soon as the resumed runtime reports one. It carries no runtime
 * epoch or revision: a choice made on it is stored for the session's next
 * runtime instead of being sent to one.
 */
export function restoredSessionConfigView(
  page: AcpChatPage | null | undefined,
  bindingId: string | null | undefined,
  modelId: string | null,
): AcpSessionConfig | null {
  const restored = page?.restoredSessionConfig;
  if (!page || !restored || page.sessionConfig) return null;
  const session = page.sessions.find(
    (candidate) =>
      candidate.id === page.currentSessionId &&
      candidate.id === restored.sageAgentSessionId,
  );
  if (
    !session ||
    session.legacy ||
    session.bindingId !== bindingId ||
    (session.modelId ?? null) !== modelId
  )
    return null;
  const { awaiting, ...config } = restored;
  return {
    ...config,
    runtimeEpoch: "",
    configRevision: "",
    restored: awaiting === "report" ? PENDING_NOTE : RESTORED_NOTE,
  };
}

/** The config the composer shows, with the page's notice about it. */
export function withSessionConfigNotice(
  config: AcpSessionConfig | null,
  notice: string | null | undefined,
): AcpSessionConfig | null {
  return config && notice ? { ...config, notice } : config;
}
