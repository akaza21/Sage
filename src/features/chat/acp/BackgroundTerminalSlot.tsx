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
 * background terminals. It lists them when it mounts, each time the latest
 * Turn ends and after each stop, never on a timer; the strip loads only
 * while one runs.
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
  const latest = page.turns.at(-1);
  const settled =
    !latest || terminalTurnStates.has(latest.state) ? (latest?.id ?? "") : null;
  const [listed, setListed] = useState<Listed | null>(null);
  const [stops, setStops] = useState(0);
  // Only the newest listing lands; a stop makes any listing before it stale.
  const sequence = useRef(0);
  useEffect(() => {
    if (!request || settled === null) return;
    const started = ++sequence.current;
    const land = (terminals: readonly BackgroundTerminal[] | null) =>
      started === sequence.current &&
      setListed(terminals && { request, terminals });
    acpBackgroundTerminals.list(request).then(land, () => land(null));
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
