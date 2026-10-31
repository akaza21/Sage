import { useEffect, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, ProjectGeneration } from "../../domain/acp";
import { getGoalChatPage } from "../../services/acpGoalDesktop";

const MAX_INHERITED_TURNS = 20;
const MAX_SEEK_PAGES = 3;
type History = {
  revision: number;
  owner: string;
  scope: string;
  page: AcpChatPage | null;
  remaining: AcpChatPage["turns"];
  cursor: AcpChatPage["nextCursor"];
  error?: string;
  missing?: boolean;
};

/** Bounded seek batches; reading older history replaces, rather than appends, its 20 mounted turns. */
export function useInheritedHistory(parentChatId: string, forkTurnId?: string) {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const generation = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const owner = JSON.stringify([projectId, parentChatId, forkTurnId]);
  const scope = JSON.stringify([
    projectId,
    generation,
    parentChatId,
    forkTurnId,
  ]);
  const [history, setHistory] = useState<History | null>(null);
  const [request, setRequest] = useState({
    scope,
    cursor: null as AcpChatPage["nextCursor"],
    seek: true,
    revision: 0,
  });
  const current = history?.scope === scope ? history : null;
  const revision = request.scope === scope ? request.revision : 0;
  const loading = Boolean(
    projectId && forkTurnId && (!current || current.revision !== revision),
  );
  useEffect(() => {
    if (!projectId || !forkTurnId) return;
    let live = true;
    const cursor = request.scope === scope ? request.cursor : null;
    const seek = request.scope !== scope || request.seek;
    void (async () => {
      let next = cursor;
      try {
        const visited = new Set<string | null>();
        for (let read = 0; read < MAX_SEEK_PAGES; read++) {
          if (visited.has(next))
            throw new Error("Parent history cursor did not advance.");
          visited.add(next);
          const page = await getGoalChatPage({
            projectId: projectId as AcpChatPage["projectId"],
            projectGeneration: String(generation) as ProjectGeneration,
            chatId: parentChatId as AcpChatPage["chatId"],
            cursor: next,
          });
          if (!live) return;
          if (page.projectId !== projectId || page.chatId !== parentChatId)
            throw new Error("Parent history belongs to another chat.");
          const end = seek
            ? page.turns.findIndex((turn) => turn.id === forkTurnId) + 1
            : page.turns.length;
          if (end) {
            const start = Math.max(0, end - MAX_INHERITED_TURNS);
            setHistory({
              revision,
              owner,
              scope,
              page: { ...page, turns: page.turns.slice(start, end) },
              remaining: page.turns.slice(0, start),
              cursor: page.nextCursor,
            });
            return;
          }
          next = page.nextCursor;
          if (next === null) {
            setHistory({
              revision,
              owner,
              scope,
              page: null,
              remaining: [],
              cursor: null,
              missing: true,
            });
            return;
          }
        }
        setHistory({
          revision,
          owner,
          scope,
          page: null,
          remaining: [],
          cursor: next,
        });
      } catch (error) {
        if (live)
          setHistory((prior) => ({
            revision,
            owner,
            scope,
            page: prior?.scope === scope ? prior.page : null,
            remaining: prior?.scope === scope ? prior.remaining : [],
            cursor: next,
            error:
              error instanceof Error
                ? error.message
                : "Could not load parent history.",
          }));
      }
    })();
    return () => {
      live = false;
    };
  }, [
    projectId,
    generation,
    parentChatId,
    forkTurnId,
    scope,
    owner,
    request,
    revision,
  ]);

  function loadEarlier() {
    if (!current) return;
    if (current.remaining.length && current.page && !current.error) {
      const start = Math.max(0, current.remaining.length - MAX_INHERITED_TURNS);
      setHistory({
        ...current,
        page: { ...current.page, turns: current.remaining.slice(start) },
        remaining: current.remaining.slice(0, start),
      });
    } else {
      setRequest({
        scope,
        cursor: current.cursor,
        seek: !current.page,
        revision: request.revision + 1,
      });
    }
  }
  return {
    page: current
      ? current.page
      : history?.owner === owner
        ? history.page
        : null,
    loading,
    error: loading ? undefined : current?.error,
    missing: current?.missing || !forkTurnId || !projectId,
    earlier: Boolean(current?.remaining.length || current?.cursor),
    loadEarlier,
  };
}
