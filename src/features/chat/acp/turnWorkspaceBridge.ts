import { useEffect, useState } from "react";

import type { AcpChatPage } from "../../../domain/acp";
import { useWorkbenchStore } from "../../../app/store";
import type { TurnWorkspaceChange } from "./turnWorkspaceChanges";

const NONE: readonly TurnWorkspaceChange[] = [];

/**
 * AP-B B5: the workspace changes of the page's latest terminal turns. The
 * loader is imported on demand so it stays out of the initial Agent bundle.
 */
export function useTurnWorkspaceChanges(
  page: AcpChatPage,
  enabled: boolean,
): readonly TurnWorkspaceChange[] {
  const generation = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const [changes, setChanges] = useState(NONE);
  const terminalKey = page.turns
    .map((turn) => `${turn.id}:${turn.state}`)
    .join("\0");
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    void import("./turnWorkspaceChanges")
      .then((module) =>
        module.loadTurnWorkspaceChanges(page, String(generation)),
      )
      .then((loaded) => {
        if (active) setChanges(loaded.length ? loaded : NONE);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    enabled,
    page.projectId,
    page.chatId,
    page.currentSessionId,
    generation,
    terminalKey,
  ]);
  return changes;
}
