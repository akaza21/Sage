import { useEffect, useState } from "react";
import type { AcpChatPage, AcpItem } from "../domain/acp";
import { ItemView } from "../features/chat/acp/TurnActivityItem";
import {
  getAcpChildTranscript,
  type ChildTranscript,
} from "../services/acpChildTranscript";
import { projectTaskResources } from "./agentArtifactTabPresentation";

const MAX_CHILD_ACTIVITIES = 32;
const MAX_VISIBLE_CHILD_ITEMS = 256;

function childActivityLabel(item: AcpItem): string {
  if (item.kind === "toolCall") return item.payload.presentation.label;
  if (item.kind === "collaboration") return item.payload.label;
  if (item.kind === "thoughtSummary") return "Thought summary";
  if (item.kind === "plan") return "Plan";
  if (item.kind === "error") return item.payload.code;
  return item.kind === "text" ? "Message" : "Activity";
}

export function SubagentSurface({
  page,
  projectGeneration,
  itemId,
  agentKey,
}: {
  page: AcpChatPage;
  projectGeneration?: string;
  itemId: string;
  agentKey?: string;
}) {
  const turn = page.turns.find((candidate) =>
    candidate.items.some((item) => item.id === itemId),
  );
  const item = turn?.items.find((candidate) => candidate.id === itemId);
  const sessionId = turn?.sessionId;
  const runtimeEpoch = turn?.runtimeEpoch;
  const session = page.sessions?.find(
    (candidate) => candidate.id === turn?.sessionId,
  );
  const keys =
    item?.kind === "collaboration" ? (item.payload.agentKeys ?? []) : [];
  const attributedKey =
    keys.length === 1 &&
    item?.kind === "collaboration" &&
    (!item.payload.agentKey || item.payload.agentKey === keys[0])
      ? keys[0]
      : keys.length === 0 && item?.kind === "collaboration"
        ? item.payload.agentKey
        : null;
  const superseded =
    !!turn &&
    page.currentSessionId !== undefined &&
    page.currentSessionId !== turn.sessionId;
  const canRead =
    !!turn &&
    item?.kind === "collaboration" &&
    !superseded &&
    !!session?.capabilities?.childTranscriptRead &&
    !!projectGeneration &&
    !!agentKey &&
    attributedKey === agentKey &&
    (session.runtimeEpoch === null ||
      session.runtimeEpoch === turn.runtimeEpoch);
  const requestKey = canRead
    ? [
        page.projectId,
        projectGeneration,
        page.chatId,
        page.projectionRevision,
        turn.sessionId,
        turn.runtimeEpoch,
        itemId,
        agentKey,
      ].join("\u0000")
    : "";
  const [read, setRead] = useState<{
    key: string;
    value: ChildTranscript | "loading" | "error";
  } | null>(null);
  useEffect(() => {
    if (
      !canRead ||
      !sessionId ||
      !runtimeEpoch ||
      !projectGeneration ||
      !agentKey
    )
      return;
    let active = true;
    getAcpChildTranscript({
      projectId: page.projectId,
      projectGeneration,
      chatId: page.chatId,
      expectedProjectionRevision: page.projectionRevision,
      sessionId,
      runtimeEpoch,
      itemId,
      agentKey,
    })
      .then((value) => {
        if (active) setRead({ key: requestKey, value });
      })
      .catch(() => {
        // Only malformed successful responses throw; request failures are typed.
        if (active) setRead({ key: requestKey, value: "error" });
      });
    return () => {
      active = false;
    };
  }, [
    requestKey,
    canRead,
    page.projectId,
    page.chatId,
    page.projectionRevision,
    projectGeneration,
    sessionId,
    runtimeEpoch,
    itemId,
    agentKey,
  ]);
  if (!turn || item?.kind !== "collaboration") return null;

  const label =
    item.payload.receiverLabels.length === 1
      ? item.payload.receiverLabels[0]
      : item.payload.operation === "started"
        ? item.payload.label
        : "Subagent";
  const states = item.payload.agentStates ?? [];
  const recent = agentKey
    ? projectTaskResources(page, page.chatId).recentAgents
    : [];
  const latest = agentKey
    ? (
        recent.find((agent) => agent.id === `${itemId}:${agentKey}`) ??
        (item.payload.operation === "started"
          ? recent.filter((agent) => agent.agentKey === agentKey).at(-1)
          : null)
      )?.status
    : null;
  const status = latest
    ? latest === "starting"
      ? "Starting when observed"
      : latest === "working"
        ? "Working when observed"
        : latest === "done"
          ? "Done"
          : latest === "stopped"
            ? "Stopped"
            : "Failed"
    : states.some(([, state]) => state === "running" || state === "pendingInit")
      ? "Working when observed"
      : states.length > 0 && states.every(([, state]) => state === "completed")
        ? "Done"
        : item.payload.operation === "interrupted"
          ? "Stopped"
          : "Observed";

  const childIds = new Set<string>();
  for (const activity of turn.activities) {
    if (activity.parentItemId === itemId) childIds.add(activity.id);
  }
  for (let depth = 0; depth < 3; depth += 1) {
    for (const activity of turn.activities) {
      if (activity.parentActivityId && childIds.has(activity.parentActivityId))
        childIds.add(activity.id);
    }
  }
  const childItems = turn.items
    .filter(
      (candidate) =>
        candidate.id !== itemId &&
        candidate.activityId &&
        childIds.has(candidate.activityId),
    )
    .slice(0, MAX_CHILD_ACTIVITIES);
  const state = read?.key === requestKey ? read.value : "loading";
  const retainedTurns =
    typeof state === "object" && "turns" in state ? state.turns : [];
  const { visibleTurns, visibleCount } = retainedTurns.reduce(
    (result, childTurn) => {
      const items = childTurn.items.slice(
        0,
        Math.max(0, MAX_VISIBLE_CHILD_ITEMS - result.visibleCount),
      );
      return {
        visibleTurns: [...result.visibleTurns, { ...childTurn, items }],
        visibleCount: result.visibleCount + items.length,
      };
    },
    { visibleTurns: [] as typeof retainedTurns, visibleCount: 0 },
  );
  const retainedCount = retainedTurns.reduce(
    (count, childTurn) => count + childTurn.items.length,
    0,
  );
  return (
    <section className="subagent-surface" aria-label="Subagent activity">
      <header>
        <span className="subagent-surface__avatar" aria-hidden="true">
          ✦
        </span>
        <div>
          <h1>{label}</h1>
          <span role="status">{status}</span>
        </div>
      </header>
      {superseded ? (
        <p>
          This subagent’s activity is from an earlier session and isn’t
          available here.
        </p>
      ) : canRead ? (
        <div className="subagent-surface__transcript" aria-live="polite">
          {state === "loading" ? <p>Loading child transcript…</p> : null}
          {state === "error" ? (
            <p>Child transcript is unavailable. Try reopening this view.</p>
          ) : null}
          {typeof state === "object" && !("turns" in state) ? (
            <p>
              {
                {
                  tooLarge: "Child transcript is too large to display.",
                  stale:
                    "Child transcript read is stale. Reopen this view after the chat updates.",
                  rejected:
                    "Child transcript read was rejected. No transcript was loaded.",
                  recoveryRequired:
                    "Child transcript read needs recovery. Reload the session.",
                  unavailable:
                    "Child transcript is unavailable. Try reopening this view.",
                }[state.kind]
              }
            </p>
          ) : null}
          {typeof state === "object" && "turns" in state ? (
            <>
              <p>
                {state.kind === "current"
                  ? "Current child transcript"
                  : "From an earlier session"}
              </p>
              {state.retentionLimited ? (
                <p>Some later child activity was not retained.</p>
              ) : null}
              {retainedCount > visibleCount ? (
                <p>
                  Showing the first {MAX_VISIBLE_CHILD_ITEMS} retained child
                  items.
                </p>
              ) : null}
              {visibleTurns.every(
                (childTurn) => childTurn.items.length === 0,
              ) ? (
                <p>No child activity was captured.</p>
              ) : (
                visibleTurns.map((childTurn, index) => (
                  <section
                    key={childTurn.id}
                    className="acp-turn"
                    aria-label={`Child Turn ${index + 1}`}
                  >
                    <p>
                      Turn {index + 1} · {childTurn.status}
                    </p>
                    <div className="acp-turn__items">
                      {childTurn.items.map((child) => (
                        <ItemView
                          key={child.id}
                          item={child}
                          interactive={false}
                          neutralTextRole={
                            child.kind === "text" &&
                            child.payload.role === "system"
                          }
                        />
                      ))}
                    </div>
                  </section>
                ))
              )}
            </>
          ) : null}
        </div>
      ) : (
        <p>
          Bounded activity recorded in the originating Turn. A full child
          transcript is not available in this harness projection.
        </p>
      )}
      {!superseded && !canRead && childItems.length > 0 ? (
        <ol aria-label="Captured child activity">
          {childItems.map((child) => (
            <li key={child.id}>
              <span>{childActivityLabel(child)}</span>
              <small>{child.lifecycle}</small>
            </li>
          ))}
        </ol>
      ) : !superseded && !canRead ? (
        <p>No separate child activity was captured for this row.</p>
      ) : null}
      {keys.length > 1 ? (
        <p>
          This event includes {keys.length} subagents; individual child activity
          is not attributed without a matching projection.
        </p>
      ) : null}
    </section>
  );
}
