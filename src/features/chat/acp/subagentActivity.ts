import type { AcpItem } from "../../../domain/acp";

export type CollaborationProjectionItem = Extract<
  AcpItem,
  { kind: "collaboration" }
>;
export const MAX_SUBAGENT_GROUP_ITEMS = 300;
export type SubagentRowState =
  "started working" | "finished" | "failed to start" | "failed" | "stopped";
export type SubagentEntry = { key: string; label: string };
export type SubagentActivityRow = {
  state: SubagentRowState;
  entries: SubagentEntry[];
};

/** A child's stable glyph colour, shared by the row, list and child header. */
export function avatarColor(key: string): number {
  let hash = 0;
  for (let index = 0; index < key.length; index += 1)
    hash = (hash * 31 + key.charCodeAt(index)) | 0;
  return Math.abs(hash % 3);
}

/** One run of row text; `index` names a label's own target, `more` the rest. */
export type SubagentSegment = { text: string; index?: number; more?: true };

/** `A`, `A and B`, `A, B and C`, `A, B and N more`, then the verb. */
export function subagentSegments(
  labels: readonly string[],
  state: string,
): SubagentSegment[] {
  const parts: SubagentSegment[] = labels
    .flatMap((text, index) => (text ? [{ text, index }] : []))
    .slice(0, labels.length > 3 ? 2 : 3);
  const remaining = labels.length - parts.length;
  if (!parts.length)
    parts.push(
      labels.length === 1
        ? { text: "Subagent", index: 0 }
        : { text: `${labels.length} subagents` },
    );
  else if (remaining) parts.push({ text: `${remaining} more`, more: true });
  return [
    ...parts.flatMap((part, index) =>
      index
        ? [{ text: index < parts.length - 1 ? ", " : " and " }, part]
        : [part],
    ),
    { text: ` ${state}` },
  ];
}

export function subagentSummary(
  labels: readonly string[],
  state: string,
): string {
  return subagentSegments(labels, state)
    .map((segment) => segment.text)
    .join("");
}

export function collaborationRows(
  item: CollaborationProjectionItem,
): SubagentActivityRow[] {
  const {
    operation,
    agentKey,
    agentKeys = [],
    agentStates = [],
    receiverLabels,
    label,
  } = item.payload;
  const rows = new Map<SubagentRowState, SubagentEntry[]>();
  const seen = new Set<string>();
  const append = (key: string, state: SubagentRowState, name: string) => {
    if (seen.has(key)) return;
    seen.add(key);
    const entries = rows.get(state) ?? [];
    entries.push({ key, label: name.trim() === "Subagent" ? "" : name.trim() });
    rows.set(state, entries);
  };
  for (const [key, state] of agentStates.slice(0, 32)) {
    const index = agentKeys.indexOf(key);
    if (index < 0) continue;
    const name =
      receiverLabels.length === agentKeys.length ? receiverLabels[index] : "";
    if (state === "completed") append(key, "finished", name);
    else if (state === "errored" || state === "notFound")
      append(
        key,
        operation === "spawnAgent" ? "failed to start" : "failed",
        name,
      );
    else if (state === "shutdown" || state === "interrupted")
      append(key, "stopped", name);
    else if (
      (state === "running" || state === "pendingInit") &&
      operation === "spawnAgent"
    )
      append(key, "started working", name);
  }
  if (
    agentKey &&
    rows.size === 0 &&
    (operation === "started" ||
      operation === "interrupted" ||
      operation === "completed")
  )
    append(
      agentKey,
      operation === "started"
        ? "started working"
        : operation === "completed"
          ? "finished"
          : "stopped",
      label,
    );
  return [...rows].map(([state, entries]) => ({ state, entries }));
}

export function collaborationGroupState(
  item: CollaborationProjectionItem,
): SubagentRowState | null {
  const rows = collaborationRows(item);
  return rows.length === 1 ? rows[0].state : null;
}

export function collaborationRun(
  items: readonly AcpItem[],
  start: number,
): CollaborationProjectionItem[] | null {
  const item = items[start];
  if (item?.kind !== "collaboration") return null;
  const state = collaborationGroupState(item);
  if (!state) return null;
  const members = [item];
  let count = collaborationRows(item)[0].entries.length;
  while (members.length < MAX_SUBAGENT_GROUP_ITEMS) {
    const candidate = items[start + members.length];
    if (
      !candidate ||
      candidate.kind !== "collaboration" ||
      candidate.projectId !== item.projectId ||
      candidate.chatId !== item.chatId ||
      candidate.sessionId !== item.sessionId ||
      candidate.turnId !== item.turnId ||
      candidate.runtimeEpoch !== item.runtimeEpoch ||
      candidate.origin !== item.origin ||
      candidate.activityId !== item.activityId ||
      collaborationGroupState(candidate) !== state
    )
      break;
    const candidateCount = collaborationRows(candidate)[0].entries.length;
    if (count + candidateCount > MAX_SUBAGENT_GROUP_ITEMS) break;
    count += candidateCount;
    members.push(candidate);
  }
  return members;
}

export function subagentToolSummary(
  items: readonly Extract<AcpItem, { kind: "toolCall" }>[],
): string {
  const wording = {
    pending: "started working",
    waitingForApproval: "waiting for approval",
    inProgress: "started working",
    completed: "finished",
    failed: "failed",
    declined: "declined",
    cancelled: "stopped",
  } as const;
  const groups = new Map<string, string[]>();
  for (const item of items.slice(0, MAX_SUBAGENT_GROUP_ITEMS)) {
    const state = wording[item.toolState];
    const labels = groups.get(state) ?? [];
    const label = item.payload.presentation.label.trim();
    labels.push(
      ["Nested activity", "Task", "Agent", "Subagent activity"].includes(label)
        ? ""
        : label,
    );
    groups.set(state, labels);
  }
  return [...groups]
    .map(([state, labels]) => subagentSummary(labels, state))
    .join(" · ");
}
