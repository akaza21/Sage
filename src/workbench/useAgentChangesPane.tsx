import { useEffect, useRef } from "react";
import {
  sameAgentArtifactOwner,
  type AgentPaneOwner,
} from "../domain/agentArtifact";
import type { AcpChatPage } from "../domain/acp";
import {
  agentArtifactFollowRecordFor,
  useAgentArtifactFollowStore,
} from "../app/agentArtifactFollowStore";

import {
  AgentChangesView,
  AgentChangesTab,
  AgentChangesObserver,
} from "./AgentChangesViews";

export function useAgentChangesPane(
  owner: AgentPaneOwner | null,
  generation: number,
  expanded: boolean,
  onExpandedChange: ((expanded: boolean) => void) | undefined,
  page: AcpChatPage | null = null,
) {
  const record = useAgentArtifactFollowStore((follow) =>
    agentArtifactFollowRecordFor(follow.records, owner),
  );
  const request = record?.recent.find((target) => target.kind === "changes");
  const open = request?.generation === generation;
  const selected = open && record?.selected?.kind === "changes";
  const previousRequest = useRef({ owner, request });
  useEffect(() => {
    const previous = previousRequest.current;
    previousRequest.current = { owner, request };
    if (
      selected &&
      !expanded &&
      sameAgentArtifactOwner(previous.owner, owner) &&
      previous.request !== request
    )
      onExpandedChange?.(true);
  }, [selected, expanded, onExpandedChange, owner, request]);
  return {
    observer: (
      <AgentChangesObserver
        key={JSON.stringify([
          generation,
          owner?.projectId,
          owner?.chatId,
          owner?.sessionId,
        ])}
        page={page}
        generation={generation}
      />
    ),
    open,
    selected,
    tab: open ? (
      <AgentChangesTab
        selected={selected}
        select={() => {
          if (owner)
            useAgentArtifactFollowStore
              .getState()
              .pin(owner, { kind: "changes", generation });
        }}
        close={() => {
          if (owner)
            useAgentArtifactFollowStore
              .getState()
              .dismiss(owner, { kind: "changes" });
        }}
      />
    ) : null,
    surface: selected ? (
      <AgentChangesView
        key={`${generation}:${owner?.chatId}:${owner?.sessionId}`}
        chatId={owner?.chatId ?? null}
      />
    ) : null,
  };
}
