import { useEffect, useState } from "react";

import { useWorkbenchStore } from "../../../app/store";
import type {
  AcpProjectionChangedEvent,
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import {
  type ChatLiveSnapshot,
  projectChatLiveSnapshot,
} from "./chatLiveSnapshot";

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
  const active = Boolean(
    enabled && projectId && chatId && !projectTransitioning,
  );

  useEffect(() => {
    if (!active || !projectId) {
      return;
    }

    let disposed = false;
    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    let unlisten: (() => void) | null = null;
    let pending = false;

    const loadLatest = () => {
      if (disposed) return;
      pending = false;
      void acpDesktop
        .getChatPage({
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
        });
    };

    const onChange = (event: AcpProjectionChangedEvent) => {
      if (
        disposed ||
        event.projectId !== projectId ||
        event.chatId !== chatId
      ) {
        return;
      }
      if (refreshTimer || pending) return;
      pending = true;
      refreshTimer = setTimeout(() => {
        refreshTimer = null;
        loadLatest();
      }, 50);
    };

    void acpDesktop
      .subscribeProjectionChanges(onChange)
      .then((disposeListener) => {
        if (disposed) {
          disposeListener();
          return;
        }
        unlisten = disposeListener;
        loadLatest();
      })
      .catch(() => {
        loadLatest();
      });

    return () => {
      disposed = true;
      if (refreshTimer) clearTimeout(refreshTimer);
      unlisten?.();
    };
  }, [active, chatId, nativeTransitionEpoch, projectId]);

  return active && snapshot?.chatId === chatId
    ? snapshot
    : idleSnapshot(chatId);
}
