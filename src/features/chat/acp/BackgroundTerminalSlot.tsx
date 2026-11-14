import { useEffect, useRef, useState } from "react";
import { useWorkbenchStore } from "../../../app/store";
import type { AcpChatPage } from "../../../domain/acp";
import type { AcpUsageRequest } from "../../../domain/acpUsage";
import {
  acpBackgroundTerminals,
  type BackgroundTerminal,
} from "../../../services/acpBackgroundTerminals";
import { lazyParts } from "../lazyParts";
import type * as StripChunk from "./BackgroundTerminals";
import { terminalTurnStates } from "./statusSemantics";
import { useUsageRequest } from "./useUsageRequest";

const Strip = lazyParts<typeof StripChunk>(
  () => import("./BackgroundTerminals"),
);

interface Listed {
  request: AcpUsageRequest;
  terminals: readonly BackgroundTerminal[];
}

/**
 * The commands the current session left running, once it negotiated
 * background terminals. It lists them when it mounts or the runtime changes,
 * each time a Turn ends and after each stop, even while a Turn runs, but
 * never on a timer or a streaming update; the strip loads only while one
 * runs.
 */
export function BackgroundTerminalSlot({
  page,
  rootLabels,
}: {
  page: AcpChatPage;
  rootLabels: Readonly<Record<string, string>>;
}) {
  const request = useUsageRequest(
    page,
    useWorkbenchStore((state) => state.nativeTransitionEpoch),
    page.sessions.find(({ id }) => id === page.currentSessionId)?.capabilities
      ?.nativeBackgroundTerminals ?? false,
  );
  const settled = lastSettledTurn(page.turns);
  const [listed, setListed] = useState<Listed | null>(null);
  const [stops, setStops] = useState(0);
  // Only the newest listing lands; a stop makes any listing before it stale.
  // A listing that fails keeps the last one for this runtime, so its
  // terminals stay stoppable until a later listing says otherwise.
  const sequence = useRef(0);
  useEffect(() => {
    if (!request) return;
    const started = ++sequence.current;
    acpBackgroundTerminals.list(request).then(
      (terminals) =>
        started === sequence.current && setListed({ request, terminals }),
      () => undefined,
    );
  }, [request, settled, stops]);
  if (!request || listed?.request !== request || !listed.terminals.length)
    return null;
  return (
    <Strip
      part="BackgroundTerminals"
      terminals={listed.terminals}
      rootLabels={rootLabels}
      onStop={(handle: string | null) => {
        sequence.current++;
        return acpBackgroundTerminals
          .stop(request, handle)
          .finally(() => setStops((count) => count + 1));
      }}
    />
  );
}

/** The newest Turn that has ended: it moves only when a Turn ends. */
function lastSettledTurn(turns: AcpChatPage["turns"]): string | undefined {
  for (let index = turns.length - 1; index >= 0; index--) {
    const turn = turns[index];
    if (terminalTurnStates.has(turn.state)) return turn.id;
  }
  return undefined;
}
