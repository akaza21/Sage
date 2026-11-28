import { useMemo, useRef, useLayoutEffect, type RefObject } from "react";
import { FileText, Globe2 } from "../ui/leanIcons";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import { openAgentArtifact } from "../app/openAgentArtifact";
import { MenuPopover } from "../ui/MenuPopover";
import { projectTaskResources } from "./taskResourceProjection";
import { useAgentChangesState } from "./agentChangesState";
import { AgentOverviewEntries, useOverviewPage } from "./AgentPaneLauncher";
import "./agent-resources.css";

export { AgentPaneLauncher, AgentPaneAskPill } from "./AgentPaneLauncher";
export { AgentPaneViewCanvas } from "./AgentPaneViewCanvas";

export function AgentResourcesPopover({
  chatId,
  resourcesId,
  resourcesTriggerRef,
  showAllSources,
  onToggleSources,
  onDismiss,
}: {
  chatId: string;
  resourcesId: string;
  resourcesTriggerRef: RefObject<HTMLButtonElement | null>;
  showAllSources: boolean;
  onToggleSources: () => void;
  onDismiss: () => void;
}) {
  const { page, validPage } = useOverviewPage(chatId);
  const sessionKey = `${page?.currentSessionId}:${page?.sessions.find((session) => session.id === page.currentSessionId)?.runtimeEpoch}`;
  const previousSession = useRef(sessionKey);
  const focusWithin = useRef(false);
  useLayoutEffect(() => {
    const trackFocus = () => {
      focusWithin.current = !!document
        .getElementById(resourcesId)
        ?.contains(document.activeElement);
    };
    trackFocus();
    document.addEventListener("focusin", trackFocus);
    return () => document.removeEventListener("focusin", trackFocus);
  }, [resourcesId]);
  useLayoutEffect(() => {
    if (previousSession.current === sessionKey) return;
    previousSession.current = sessionKey;
    if (focusWithin.current) resourcesTriggerRef.current?.focus();
    if (showAllSources) onToggleSources();
    onDismiss();
  }, [
    sessionKey,
    resourcesId,
    resourcesTriggerRef,
    showAllSources,
    onToggleSources,
    onDismiss,
  ]);
  const { outputs, sources } = useMemo(
    () => projectTaskResources(validPage ? page : null, chatId),
    [page, chatId, validPage],
  );

  return (
    <MenuPopover
      id={resourcesId}
      className="agent-task-header__resource-panel agent-overview"
      role="dialog"
      ariaLabel="Overview"
      open
      triggerRef={resourcesTriggerRef}
      onDismiss={() => onDismiss()}
    >
      <strong className="agent-overview__heading">Overview</strong>
      <AgentOverviewEntries
        chatId={chatId}
        onDone={onDismiss}
        focusReturn={resourcesTriggerRef}
      >
        {outputs.length > 0 ? (
          <section aria-label="Outputs">
            <strong className="agent-overview__heading">Outputs</strong>
            {outputs.map((output) => (
              <button
                key={`${output.resource.rootId}:${output.label}`}
                className="agent-overview__row"
                type="button"
                aria-label={`Open output ${output.label}`}
                disabled={
                  !validPage ||
                  output.owner.sessionId !== page?.currentSessionId
                }
                title={
                  output.owner.sessionId !== page?.currentSessionId
                    ? "This output belongs to an earlier session and cannot be opened here."
                    : output.label
                }
                onClick={() => {
                  void openAgentArtifact({
                    owner: output.owner,
                    target: output.target,
                    resource: output.resource,
                    ...(output.diffRef ? { diffRef: output.diffRef } : {}),
                  }).then((opened) => {
                    if (opened) {
                      useAgentChangesState.getState().select(false);
                      useSubagentPaneStore.getState().select(false);
                      onDismiss();
                    }
                  });
                }}
              >
                <FileText size={16} aria-hidden="true" />
                <span>{output.label}</span>
              </button>
            ))}
          </section>
        ) : null}
      </AgentOverviewEntries>
      {sources.length > 0 ? (
        <section aria-label="Sources">
          <strong className="agent-overview__heading">Sources</strong>
          {(showAllSources ? sources : sources.slice(-3)).map(
            (source, index) => (
              <span
                key={source.label + ":" + index}
                className="agent-overview__row"
                title={
                  source.label + " - Recorded source; not a live browser link"
                }
              >
                <Globe2 size={16} aria-hidden="true" />
                <span>{source.label}</span>
              </span>
            ),
          )}
          {sources.length > 3 ? (
            <button
              className="agent-task-header__view-all"
              type="button"
              onClick={onToggleSources}
            >
              {showAllSources ? "Show less" : "View all"}
            </button>
          ) : null}
        </section>
      ) : null}
    </MenuPopover>
  );
}
