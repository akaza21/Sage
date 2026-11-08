import type { AcpChatPage } from "../../domain/acp";
import type {
  AcpSessionConfig,
  AcpSessionConfigNotice,
} from "../../domain/acpSessionConfig";
import { flattenChoices } from "../../domain/acpSessionConfigControls";

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

/**
 * What the composer says about a saved model the session does not run,
 * naming each model as the session's options name it.
 */
export function sessionConfigNoticeText(
  config: AcpSessionConfig,
  notice: AcpSessionConfigNotice,
): string {
  const option = config.configOptions.find(
    (candidate) => candidate.id === notice.configId,
  );
  const name = (value: string) =>
    (option && option.type === "select"
      ? flattenChoices(option).find((choice) => choice.value === value)?.name
      : undefined) || value;
  const saved = name(notice.saved);
  const current = name(notice.current);
  return notice.reason === "rejected"
    ? `The harness refused the saved model ${saved}; this chat uses ${current}.`
    : `The saved model ${saved} isn't offered any more; this chat uses ${current}.`;
}

/** The config the composer shows, with the page's notice about it. */
export function withSessionConfigNotice(
  config: AcpSessionConfig | null,
  notice: AcpSessionConfigNotice | null | undefined,
): AcpSessionConfig | null {
  return config && notice
    ? { ...config, notice: sessionConfigNoticeText(config, notice) }
    : config;
}
