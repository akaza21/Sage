import { useEffect, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, ProjectGeneration } from "../../domain/acp";
import { getGoalChatPage } from "../../services/acpGoalDesktop";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { GitBranch } from "../../ui/leanIcons";
import "./fork-history.css";
export function ForkDivider({
  parentChatId,
  forkTurnId,
  forkChatId,
}: {
  parentChatId: string;
  forkTurnId?: string;
  forkChatId?: string;
}) {
  const open = useWorkbenchStore((state) =>
    state.tabs.some((tab) => tab.kind === "chat" && tab.id === parentChatId),
  );
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const inherited = useInheritedHistory(parentChatId, forkTurnId, forkChatId);
  return (
    <>
      <p className="acp-history-notice">This chat shares the project folder.</p>
      {inherited ? (
        <InheritedHistory
          page={inherited}
          earlier={inherited.earlier}
          label="History from the parent chat"
        />
      ) : null}
      <div className="chat-fork-divider">
        <GitBranch size={14} />
        {open ? (
          <button
            type="button"
            className="chat-fork-divider__link"
            onClick={() => void activateTab(parentChatId)}
          >
            Continued from chat
          </button>
        ) : (
          <span>Continued from an archived chat</span>
        )}
      </div>
    </>
  );
}

export function InheritedHistory({
  page,
  earlier,
  label,
}: {
  page: AcpChatPage;
  earlier: boolean;
  label: string;
}) {
  return (
    <div className="chat-fork-inherited" aria-label={label} role="group">
      {earlier ? (
        <p className="acp-history-notice">Earlier history isn&apos;t shown.</p>
      ) : null}
      <LazyModuleBoundary
        label="Inherited history"
        load={loadInheritedTurns}
        render={(InheritedTurns) => <InheritedTurns page={page} />}
      />
    </div>
  );
}

const MAX_INHERITED_TURNS = 20;

const loadInheritedTurns = () =>
  import("./acp/AcpChatPresentation").then(({ InheritedTurns }) => ({
    default: InheritedTurns,
  }));

type Inherited = AcpChatPage & {
  forkTurnId: string;
  earlier: boolean;
};

function useInheritedHistory(
  parentChatId: string,
  forkTurnId: string | undefined,
  forkChatId?: string,
) {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const generation = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const [inherited, setInherited] = useState<Inherited | null>(null);
  useEffect(() => {
    if (!projectId || !forkTurnId) return;
    let live = true;
    void getGoalChatPage({
      projectId: projectId as AcpChatPage["projectId"],
      projectGeneration: String(generation) as ProjectGeneration,
      chatId: parentChatId as AcpChatPage["chatId"],
      cursor: null,
      ...(forkChatId
        ? { forkHistoryFor: forkChatId as AcpChatPage["chatId"] }
        : {}),
    })
      .then((page) => {
        if (!live || page.chatId !== parentChatId) return;
        const end = forkChatId
          ? page.turns.length
          : page.turns.findIndex((turn) => turn.id === forkTurnId) + 1;
        if (!end) return setInherited(null);
        const start = Math.max(0, end - MAX_INHERITED_TURNS);
        setInherited({
          ...page,
          forkTurnId,
          earlier: start > 0 || page.nextCursor !== null,
          turns: page.turns.slice(start, end),
        });
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [projectId, generation, parentChatId, forkTurnId, forkChatId]);
  return inherited?.chatId === parentChatId &&
    inherited.forkTurnId === forkTurnId
    ? inherited
    : null;
}
