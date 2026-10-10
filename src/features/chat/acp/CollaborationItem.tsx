import { useId } from "react";

import {
  attributedSubagentKey,
  useSubagentPaneStore,
} from "../../../app/subagentPaneStore";
import {
  collaborationRows,
  MAX_SUBAGENT_GROUP_ITEMS,
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

export function CollaborationItem({
  item,
  items = [item],
}: {
  item: CollaborationProjectionItem;
  items?: readonly CollaborationProjectionItem[];
}) {
  const descriptionId = useId();
  const status = collaborationStateLabels[item.lifecycle];
  const rows = new Map<SubagentRowState, Map<string, SubagentEntry>>();
  for (const member of items.slice(0, MAX_SUBAGENT_GROUP_ITEMS)) {
    for (const row of collaborationRows(member)) {
      const entries = rows.get(row.state) ?? new Map();
      for (const entry of row.entries) {
        if (
          (entry.label || !entries.has(entry.key)) &&
          entries.size < MAX_SUBAGENT_GROUP_ITEMS
        )
          entries.set(entry.key, entry);
      }
      rows.set(row.state, entries);
    }
  }
  if (rows.size > 0) {
    return (
      <>
        {[...rows].map(([state, entries]) => {
          const agents = [...entries.values()];
          const text = subagentSummary(
            agents.map((agent) => agent.label),
            state,
          );
          const source =
            agents.length === 1
              ? items.find((member) =>
                  attributedSubagentKey(member, [agents[0].key]),
                )
              : undefined;
          const agentKey = source ? agents[0].key : null;
          const Row = agentKey ? "button" : "div";
          return (
            <Row
              key={state}
              className="acp-subagent-row"
              {...(source && agentKey
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
                        agentKey,
                      }),
                  }
                : {})}
            >
              <span className="acp-subagent-row__glyphs" aria-hidden="true">
                {agents.slice(0, 3).map((agent) => (
                  <span
                    key={agent.key}
                    className={`acp-subagent-row__glyph acp-subagent-row__glyph--${avatarColor(agent.key)}`}
                  >
                    {state === "finished" ? "●" : "✦"}
                  </span>
                ))}
              </span>
              <span
                role="status"
                aria-label={text}
                className={
                  state === "started working" ? "acp-live-shimmer" : undefined
                }
              >
                {text}
              </span>
            </Row>
          );
        })}
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
      {item.payload.receiverLabels.length > 0 ? (
        <p className="acp-collaboration__receivers">
          Participants: {item.payload.receiverLabels.join(", ")}
        </p>
      ) : null}
    </article>
  );
}
