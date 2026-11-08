import { useMemo } from "react";
import type { ProjectGeneration } from "../../../domain/acp";
import type { AcpChatPage } from "../../../domain/acpProjection";
import type { AcpUsageRequest } from "../../../domain/acpUsage";

/** The live runtime whose usage limits the context meter can show. */
export function useUsageRequest(
  page: AcpChatPage | null | undefined,
  projectGeneration: number,
): AcpUsageRequest | null {
  const session = page?.sessions.find(
    (candidate) => candidate.id === page.currentSessionId,
  );
  const usage = page?.usage ?? false;
  const projectId = page?.projectId;
  const chatId = page?.chatId;
  const sessionId = session?.id;
  const runtimeEpoch = session?.runtimeEpoch;
  return useMemo(() => {
    if (!usage || !projectId || !chatId || !sessionId || !runtimeEpoch) {
      return null;
    }
    return {
      projectId,
      projectGeneration: String(projectGeneration) as ProjectGeneration,
      chatId,
      sessionId,
      runtimeEpoch,
    };
  }, [usage, projectId, chatId, sessionId, runtimeEpoch, projectGeneration]);
}
