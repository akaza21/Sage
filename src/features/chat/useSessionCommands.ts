import { useEffect, useState } from "react";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import type {
  AcpChatPage,
  RuntimeEpoch,
  SageAgentSessionId,
} from "../../domain/acp";
import {
  acpCommandsDesktop,
  type AcpCommandList,
} from "../../services/acpCommandsDesktop";
import { createClientIntentFactory } from "./acp/clientIntent";
import { currentContext } from "./acp/useAcpChatActions";
const nextIntentId = createClientIntentFactory();
const LOADING: SessionCommandsLoad = { kind: "loading" };

export type SessionCommandsLoad =
  | { kind: "loading" }
  | { kind: "ready"; owner: string; list: AcpCommandList }
  | { kind: "error"; owner: string; message: string };

function liveOwnerKey(page: AcpChatPage | null): string {
  const session = page?.sessions.find(
    (item) => item.id === page.currentSessionId,
  );
  return session?.runtimeEpoch &&
    session.residency === "hot" &&
    (session.state === "ready" || session.state === "active")
    ? JSON.stringify([session.id, session.runtimeEpoch])
    : "";
}

export function useSessionCommands(
  boundChatId: string | undefined,
): SessionCommandsLoad | null {
  const owner = useStore(acpProjectionStoreFor(boundChatId), (state) =>
    liveOwnerKey(state.page),
  );
  const [load, setLoad] = useState<SessionCommandsLoad>(LOADING);
  useEffect(() => {
    if (!owner) return;
    const [sessionId, runtimeEpoch] = JSON.parse(owner) as [
      SageAgentSessionId,
      RuntimeEpoch,
    ];
    let disposed = false;
    let revision = 0;
    let unlisten: (() => void) | null = null;
    const read = async () => {
      if (disposed) return;
      const reading = ++revision;
      try {
        const context = currentContext(boundChatId);
        if (!context) throw new Error("This chat is not ready.");
        const list = await acpCommandsDesktop.listCommands({
          ...context.scope,
          projectGeneration: context.commandProjectGeneration,
          expectedProjectionRevision: context.page.projectionRevision,
          clientIntentId: nextIntentId(),
          sessionId,
          runtimeEpoch,
        });
        if (!disposed && reading === revision)
          setLoad({ kind: "ready", owner, list });
      } catch (error) {
        if (!disposed && reading === revision)
          setLoad({
            kind: "error",
            owner,
            message:
              error instanceof Error
                ? error.message
                : "The command list could not be read.",
          });
      }
    };
    // Subscribe before reading so a catalog update during the first read is
    // observed. Only the latest read may publish; cleanup revokes them all.
    void acpCommandsDesktop
      .subscribeCommandChanges((event) => {
        if (
          event.sageAgentSessionId === sessionId &&
          event.runtimeEpoch === runtimeEpoch
        )
          void read();
      })
      .then(
        (dispose) => {
          if (disposed) return dispose();
          unlisten = dispose;
          void read();
        },
        () => void read(),
      );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [boundChatId, owner]);
  if (!owner) return null;
  return load.kind === "loading" || load.owner === owner ? load : LOADING;
}
