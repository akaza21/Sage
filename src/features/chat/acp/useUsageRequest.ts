import { useMemo } from "react";
import type { ProjectGeneration } from "../../../domain/acp";
import type { AcpChatPage } from "../../../domain/acpProjection";
import type { AcpUsageRequest } from "../../../domain/acpUsage";

export function useUsageRequest(
  page: AcpChatPage | null | undefined,
  projectGeneration: number,
  served = page?.usage ?? false,
): AcpUsageRequest | null {
  const session = page?.sessions.find(
    (candidate) => candidate.id === page.currentSessionId,
  );
  const projectId = page?.projectId;
  const chatId = page?.chatId;
  const sessionId = session?.id;
  const runtimeEpoch = session?.runtimeEpoch;
  return useMemo(() => {
    if (!served || !projectId || !chatId || !sessionId || !runtimeEpoch) {
      return null;
    }
    return {
      projectId,
      projectGeneration: String(projectGeneration) as ProjectGeneration,
      chatId,
      sessionId,
      runtimeEpoch,
    };
  }, [served, projectId, chatId, sessionId, runtimeEpoch, projectGeneration]);
}
