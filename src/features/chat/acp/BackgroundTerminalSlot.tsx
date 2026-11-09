import { useEffect, useMemo, useState } from "react";
import { useWorkbenchStore } from "../../../app/store";
import type { AcpChatPage, ProjectGeneration } from "../../../domain/acp";
import type { AcpUsageRequest } from "../../../domain/acpUsage";
import {
  acpBackgroundTerminals,
  type BackgroundTerminal,
} from "../../../services/acpBackgroundTerminals";
import { lazyParts } from "../lazyParts";
import type * as StripChunk from "./BackgroundTerminals";
import { terminalTurnStates } from "./statusSemantics";

const Strip = lazyParts<typeof StripChunk>(
  () => import("./BackgroundTerminals"),
);

interface Listed {
  request: AcpUsageRequest;
  terminals: readonly BackgroundTerminal[];
}

/**
 * The commands the current session left running; it mounts only once the
 * session negotiated background terminals. It lists them when it mounts and
 * each time the latest Turn ends, never on a timer, and the strip loads only
 * while one runs.
 */
export function BackgroundTerminalSlot({
  page,
  rootLabels,
}: {
  page: AcpChatPage;
  rootLabels: Readonly<Record<string, string>>;
}) {
  const generation = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const session = page.sessions.find(
    (candidate) => candidate.id === page.currentSessionId,
  );
  const sessionId = session?.id;
  const runtimeEpoch = session?.runtimeEpoch;
  const { projectId, chatId } = page;
  const request = useMemo<AcpUsageRequest | null>(
    () =>
      sessionId && runtimeEpoch
        ? {
            projectId,
            projectGeneration: String(generation) as ProjectGeneration,
            chatId,
            sessionId,
            runtimeEpoch,
          }
        : null,
    [projectId, generation, chatId, sessionId, runtimeEpoch],
  );
  const latest = page.turns.at(-1);
  const settled =
    !latest || terminalTurnStates.has(latest.state) ? (latest?.id ?? "") : null;
  const [listed, setListed] = useState<Listed | null>(null);
  useEffect(() => {
    if (!request || settled === null) return;
    let current = true;
    acpBackgroundTerminals.list(request).then(
      (terminals) => current && setListed({ request, terminals }),
      () => current && setListed(null),
    );
    return () => {
      current = false;
    };
  }, [request, settled]);
  if (!request || listed?.request !== request || !listed.terminals.length)
    return null;
  return (
    <Strip
      part="BackgroundTerminals"
      request={request}
      terminals={listed.terminals}
      rootLabels={rootLabels}
      onChange={(terminals: readonly BackgroundTerminal[]) =>
        setListed({ request, terminals })
      }
    />
  );
}
