import { useEffect, useMemo, useState } from "react";

import { useWorkbenchStore } from "../../../app/store";
import type {
  AcpProjectionChangedEvent,
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { getGoalChatPage } from "../../../services/acpGoalDesktop";
import {
  type ChatLiveSnapshot,
  projectChatLiveSnapshot,
} from "./chatLiveSnapshot";
import { coalescedRefresh } from "./coalescedRefresh";

function idleSnapshot(chatId: string): ChatLiveSnapshot {
  return projectChatLiveSnapshot(chatId, null);
}

export function useAcpChatLiveSnapshot(
  chatId: string,
  enabled: boolean,
): ChatLiveSnapshot {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const [snapshot, setSnapshot] = useState<ChatLiveSnapshot | null>(null);
  const idle = useMemo(() => idleSnapshot(chatId), [chatId]);
  const active = Boolean(
    enabled && projectId && chatId && !projectTransitioning,
  );

  useEffect(() => {
    if (!active || !projectId) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | null = null;

    const refresh = coalescedRefresh(() =>
      getGoalChatPage({
        projectId: projectId as SageProjectId,
        projectGeneration: String(nativeTransitionEpoch) as ProjectGeneration,
        chatId: chatId as SageChatId,
        cursor: null,
      })
        .then((page) => {
          if (disposed || page.chatId !== chatId) return;
          setSnapshot(projectChatLiveSnapshot(chatId, page));
        })
        .catch(() => {
          if (!disposed) setSnapshot(null);
        }),
    );

    const onChange = (event: AcpProjectionChangedEvent) => {
      if (
        disposed ||
        event.projectId !== projectId ||
        event.chatId !== chatId
      ) {
        return;
      }
      refresh.schedule();
    };

    void acpDesktop
      .subscribeProjectionChanges(onChange)
      .then((disposeListener) => {
        if (disposed) {
          disposeListener();
          return;
        }
        unlisten = disposeListener;
        refresh.now();
      })
      .catch(() => {
        refresh.now();
      });

    return () => {
      disposed = true;
      refresh.dispose();
      unlisten?.();
    };
  }, [active, chatId, nativeTransitionEpoch, projectId]);

  return active && snapshot?.chatId === chatId ? snapshot : idle;
}
