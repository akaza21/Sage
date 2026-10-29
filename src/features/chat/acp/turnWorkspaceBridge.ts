import { useEffect, useState } from "react";

import type { AcpChatPage } from "../../../domain/acp";
import { useWorkbenchStore } from "../../../app/store";
import type { TurnWorkspaceLedger } from "./turnWorkspaceChanges";
import { terminalTurnStates } from "./statusSemantics";

const NONE: TurnWorkspaceLedger = { changes: [], incompleteTurnIds: new Set() };

/**
 * AP-B B5: the workspace changes of the page's latest terminal turns. The
 * loader is imported on demand so it stays out of the initial Agent bundle.
 */
export function useTurnWorkspaceChanges(
  page: AcpChatPage | null,
  enabled: boolean,
  reload = 0,
): TurnWorkspaceLedger {
  const generation = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const terminalKey = page?.turns
    .map((turn) => `${turn.id}:${turn.state}`)
    .join("\0");
  const owner = [page?.projectId, page?.chatId, generation].join("\0");
  const scope = [owner, page?.currentSessionId, terminalKey, reload].join("\0");
  const [loaded, setLoaded] = useState({ owner: "", scope: "", ledger: NONE });
  useEffect(() => {
    if (!enabled || !page) return;
    const controller = new AbortController();
    void import("./turnWorkspaceChanges")
      .then((module) =>
        module.default(page, String(generation), controller.signal),
      )
      .then((loaded) => {
        if (!controller.signal.aborted)
          setLoaded({ owner, scope, ledger: loaded });
      })
      .catch(() => undefined);
    return () => {
      controller.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    enabled,
    page?.projectId,
    page?.chatId,
    page?.currentSessionId,
    generation,
    terminalKey,
    reload,
  ]);
  if (!enabled || !page) return NONE;
  return loaded.scope === scope
    ? loaded.ledger
    : {
        changes:
          loaded.owner === owner
            ? loaded.ledger.changes.filter((change) =>
                page.turns.some((turn) => turn.id === change.turnId),
              )
            : [],
        incompleteTurnIds: new Set(
          page.turns
            .filter((turn) => terminalTurnStates.has(turn.state))
            .map((turn) => turn.id),
        ),
      };
}
