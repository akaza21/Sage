import { useEffect, useState } from "react";

import { useWorkbenchStore } from "../../../app/store";
import type { AcpChatPage } from "../../../domain/acp";
import type { TurnWorkspaceLedger } from "./turnWorkspaceChanges";

const NONE: TurnWorkspaceLedger = { changes: [], incompleteTurnIds: new Set() };

// The loader is imported on demand so it stays out of the initial Agent bundle.
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
  const [loaded, setLoaded] = useState({ owner: "", ledger: NONE });
  useEffect(() => {
    if (!enabled || !page) return;
    const controller = new AbortController();
    void import("./turnWorkspaceChanges")
      .then((module) =>
        module.default(page, String(generation), controller.signal),
      )
      .then((loaded) => {
        if (!controller.signal.aborted) setLoaded({ owner, ledger: loaded });
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
  return loaded.owner === owner ? loaded.ledger : NONE;
}
