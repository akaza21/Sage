import { memo, useId, useState } from "react";

import {
  attributedSubagentKey,
  useSubagentPaneStore,
} from "../../../app/subagentPaneStore";
import { ChevronDown } from "../../../ui/leanIcons";
import {
  collaborationRows,
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

function avatarColor(key: string): number {
  let hash = 0;
  for (let index = 0; index < key.length; index += 1)
    hash = (hash * 31 + key.charCodeAt(index)) | 0;
  return Math.abs(hash % 3);
}

type SubagentRowEntry = SubagentEntry & {
  source?: CollaborationProjectionItem;
};

function SubagentRow({
  agents,
  state,
}: {
  agents: SubagentRowEntry[];
  state: SubagentRowState;
}) {
  const [expanded, setExpanded] = useState(false);
  const text = subagentSummary(
    agents.map((agent) => agent.label),
    state,
  );
  const source = agents.length === 1 ? agents[0].source : undefined;
  const grouped = agents.length > 1 && agents.some((agent) => agent.source);
  const Row = grouped ? "summary" : source ? "button" : "div";
  const row = (
    <Row
      className="acp-subagent-row"
      aria-label={grouped ? `${text}. Show Subagents` : undefined}
      {...(source
        ? {
            type: "button" as const,
            "aria-label": `${text}. Open Subagent`,
            onClick: () =>
              useSubagentPaneStore.getState().show({
                owner: {
                  projectId: source.projectId,
                  chatId: source.chatId,
                  sessionId: source.sessionId,
                },
                itemId: source.id,
                agentKey: agents[0].key,
              }),
          }
        : {})}
    >
      <span className="acp-subagent-row__glyphs" aria-hidden="true">
        {agents.slice(0, 3).map((agent) => (
          <span
            key={agent.key}
            className={`acp-subagent-row__glyph acp-subagent-row__glyph--${avatarColor(agent.key)}`}
            data-state={state === "started working" ? "working" : state}
          >
            {agent.label ? [...agent.label][0].toUpperCase() : "✦"}
            {state === "finished" ? (
              <span className="acp-subagent-row__done">✓</span>
            ) : null}
          </span>
        ))}
      </span>
      <span
        role="status"
        aria-label={text}
        className={state === "started working" ? "acp-live-shimmer" : undefined}
      >
        {text}
      </span>
      {grouped ? (
        <ChevronDown
          className="acp-tool-rollup__chevron"
          aria-hidden="true"
          size={14}
        />
      ) : null}
    </Row>
  );
  return grouped ? (
    <details
      className="acp-tool-rollup"
      onToggle={(event) => setExpanded(event.currentTarget.open)}
    >
      {row}
      {expanded ? (
        <div className="acp-tool-rollup__items">
          {agents.map((agent) => (
            <SubagentRow key={agent.key} agents={[agent]} state={state} />
          ))}
        </div>
      ) : null}
    </details>
  ) : (
    row
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
  const rows = new Map<SubagentRowState, Map<string, SubagentRowEntry>>();
  for (const member of items) {
    for (const row of collaborationRows(member)) {
      const entries = rows.get(row.state) ?? new Map();
      for (const entry of row.entries) {
        const previous = entries.get(entry.key);
        const source = attributedSubagentKey(member, [entry.key])
          ? member
          : undefined;
        entries.set(entry.key, {
          ...(entry.label || !previous ? entry : previous),
          source:
            source?.payload.agentKey && !previous?.source?.payload.agentKey
              ? source
              : (previous?.source ?? source),
        });
      }
      rows.set(row.state, entries);
    }
  }
  if (rows.size > 0) {
    return (
      <>
        {[...rows].map(([state, entries]) => (
          <SubagentRow
            key={state}
            agents={[...entries.values()]}
            state={state}
          />
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
          item.revision === rightItems[index].revision,
      )
    );
  },
);
