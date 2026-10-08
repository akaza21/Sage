import type { AcpChatPage, AcpMutationResult } from "../../../domain/acp";
import type {
  AcpProjectionScope,
  ProjectionCommitDisposition,
} from "../../../app/acpProjectionStore";
import type { AcpActionContext } from "./acpActionController";

export function sameScope(
  context: AcpActionContext | null,
  expected: AcpActionContext,
): boolean {
  return (
    context?.scope.projectId === expected.scope.projectId &&
    context.scope.projectGeneration === expected.scope.projectGeneration &&
    context.scope.chatId === expected.scope.chatId &&
    context.ownershipEpoch === expected.ownershipEpoch &&
    context.commandProjectGeneration === expected.commandProjectGeneration
  );
}

export function createActionProjectionCommit(dependencies: {
  getContext: () => AcpActionContext | null;
  applyAuthoritativePage: (
    scope: AcpProjectionScope,
    page: AcpChatPage,
  ) => ProjectionCommitDisposition;
}) {
  return (
    captured: AcpActionContext,
    result: AcpMutationResult,
  ): AcpChatPage | null => {
    const current = dependencies.getContext();
    if (!sameScope(current, captured)) return null;
    const previous = current!.page;
    const incoming = result.projection;
    const previousEpoch = previous.sessions.find(
      (session) => session.id === previous.currentSessionId,
    )?.runtimeEpoch;
    const incomingEpoch = incoming.sessions.find(
      (session) => session.id === incoming.currentSessionId,
    )?.runtimeEpoch;
    const page =
      !incoming.goal &&
      previous.goal &&
      previous.currentSessionId === incoming.currentSessionId &&
      previousEpoch &&
      previousEpoch === incomingEpoch
        ? { ...incoming, goal: previous.goal }
        : incoming;
    const disposition = dependencies.applyAuthoritativePage(
      captured.scope,
      page,
    );
    return disposition === "accepted" ? page : null;
  };
}
