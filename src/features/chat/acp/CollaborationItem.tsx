import { memo, useId } from "react";

import {
  attributedSubagentKey,
  useSubagentPaneStore,
} from "../../../app/subagentPaneStore";
import {
  avatarColor,
  collaborationRows,
  subagentSegments,
  subagentSummary,
  type CollaborationProjectionItem,
  type SubagentEntry,
  type SubagentRowState,
} from "./subagentActivity";

const collaborationStateLabels = {
  started: "Observed",
  streaming: "Active",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
  unsupported: "Unsupported",
} as const;

type SubagentRowEntry = SubagentEntry & {
  source?: CollaborationProjectionItem;
  state?: SubagentRowState;
};

/** Each name opens its own child; "N more" opens the Subagents list. */
function SubagentRow({
  agents,
  state,
}: {
  agents: SubagentRowEntry[];
  state: SubagentRowState;
}) {
  const labels = agents.map((agent) => agent.label);
  const segments = subagentSegments(labels, state);
  const finished = agents.filter((agent) => agent.state === "finished");
  const status =
    state === "started working" && finished.length
      ? `${subagentSummary(
          agents
            .filter((agent) => agent.state !== "finished")
            .map((agent) => agent.label),
          state,
        )}; ${subagentSummary(
          finished.map((agent) => agent.label),
          "finished",
        )}`
      : subagentSummary(labels, state);
  const target = (agent?: SubagentRowEntry) =>
    agent?.source && {
      owner: {
        projectId: agent.source.projectId,
        chatId: agent.source.chatId,
        sessionId: agent.source.sessionId,
      },
      itemId: agent.source.id,
      agentKey: agent.key,
    };
  return (
    <div className="acp-subagent-row" data-state={state}>
      <span className="acp-subagent-row__glyphs" aria-hidden="true">
        {agents.slice(0, 3).map((agent) => (
          <span
            key={agent.key}
            className={`acp-subagent-row__glyph acp-subagent-row__glyph--${avatarColor(agent.key)}`}
          />
        ))}
      </span>
      <span role="status" aria-label={status}>
        {segments.map((segment, index) => {
          const open = target(
            segment.more
              ? agents.find((agent) => agent.source)
              : agents[segment.index ?? -1],
          );
          return open ? (
            <button
              key={index}
              type="button"
              className="acp-subagent-name"
              aria-label={
                segment.more
                  ? `Show ${segment.text} subagents`
                  : `Open subagent ${segment.text}`
              }
              onClick={() =>
                useSubagentPaneStore.getState().show(open, segment.more)
              }
            >
              {segment.text}
            </button>
          ) : (
            segment.text
          );
        })}
      </span>
    </div>
  );
}

function CollaborationItemContent({
  item,
  items = [item],
}: {
  item: CollaborationProjectionItem;
  items?: readonly CollaborationProjectionItem[];
}) {
  const descriptionId = useId();
  const status = collaborationStateLabels[item.lifecycle];
  const participants = item.payload.receiverLabels.filter((label) =>
    label.trim(),
  );
  const agents = new Map<
    string,
    SubagentRowEntry & { state: SubagentRowState }
  >();
  for (const member of items) {
    for (const row of collaborationRows(member)) {
      for (const entry of row.entries) {
        const previous = agents.get(entry.key);
        // A grouped event without its own child identity still names each
        // declared key; the pane routes that key to the child's own spawn.
        const source =
          attributedSubagentKey(member, [entry.key]) ||
          (!member.payload.agentKey &&
            !row.state.startsWith("failed") &&
            member.payload.agentKeys?.includes(entry.key))
            ? member
            : undefined;
        agents.set(entry.key, {
          ...(entry.label || !previous ? entry : previous),
          state: row.state,
          source:
            source?.payload.agentKey && !previous?.source?.payload.agentKey
              ? source
              : (previous?.source ?? source),
        });
      }
    }
  }
  // Finished children stay in the run's working row until none is live.
  const working = [...agents.values()].some(
    (agent) => agent.state === "started working",
  );
  const rows = new Map<SubagentRowState, SubagentRowEntry[]>();
  for (const agent of agents.values()) {
    const state =
      working && agent.state === "finished" ? "started working" : agent.state;
    const entries = rows.get(state) ?? [];
    entries.push(agent);
    rows.set(state, entries);
  }
  if (rows.size > 0) {
    return (
      <>
        {[...rows].map(([state, entries]) => (
          <SubagentRow key={state} agents={entries} state={state} />
        ))}
      </>
    );
  }
  return (
    <article
      className="acp-collaboration"
      role="group"
      aria-label={`Harness collaboration: ${item.payload.label}`}
      aria-describedby={descriptionId}
    >
      <span className="sr-only" id={descriptionId}>
        Read-only activity owned and scheduled by the harness
      </span>
      <header>
        <span className="acp-collaboration__identity">
          <span className="acp-collaboration__kind">Collaboration</span>
          <strong
            className="acp-collaboration__label"
            title={item.payload.label}
          >
            {item.payload.label}
          </strong>
        </span>
        <span
          className="acp-collaboration__status"
          role="status"
          aria-label={status}
          aria-live="off"
          data-lifecycle={item.lifecycle}
        >
          {status}
        </span>
      </header>
      <p>{item.payload.summary}</p>
      {participants.length > 0 ? (
        <p className="acp-collaboration__receivers">
          Participants: {participants.join(", ")}
        </p>
      ) : null}
    </article>
  );
}

export const CollaborationItem = memo(
  CollaborationItemContent,
  (left, right) => {
    const leftItems = left.items ?? [left.item];
    const rightItems = right.items ?? [right.item];
    return (
      left.item.id === right.item.id &&
      left.item.revision === right.item.revision &&
      leftItems.length === rightItems.length &&
      leftItems.every(
        (item, index) =>
          item.id === rightItems[index].id &&
          item.revision === rightItems[index].revision &&
          item.payload.receiverLabels.join("\0") ===
            rightItems[index].payload.receiverLabels.join("\0"),
      )
    );
  },
);
