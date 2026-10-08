import type { AcpChatPage, AcpItem } from "../domain/acp";
import { projectTaskResources } from "./agentArtifactTabPresentation";

const MAX_CHILD_ACTIVITIES = 32;

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
  itemId,
  agentKey,
}: {
  page: AcpChatPage;
  itemId: string;
  agentKey?: string;
}) {
  const turn = page.turns.find((candidate) =>
    candidate.items.some((item) => item.id === itemId),
  );
  const item = turn?.items.find((candidate) => candidate.id === itemId);
  if (!turn || item?.kind !== "collaboration") return null;

  const keys = item.payload.agentKeys ?? [];
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
      <p>
        Bounded activity recorded in the originating Turn. A full child
        transcript is not available in this harness projection.
      </p>
      {childItems.length > 0 ? (
        <ol aria-label="Captured child activity">
          {childItems.map((child) => (
            <li key={child.id}>
              <span>{childActivityLabel(child)}</span>
              <small>{child.lifecycle}</small>
            </li>
          ))}
        </ol>
      ) : (
        <p>No separate child activity was captured for this row.</p>
      )}
      {keys.length > 1 ? (
        <p>
          This event includes {keys.length} subagents; individual child activity
          is not attributed without a matching projection.
        </p>
      ) : null}
    </section>
  );
}
