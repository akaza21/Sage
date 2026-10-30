import { useEffect, useRef, useState } from "react";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import type { AcpChatPage, AcpItem } from "../domain/acp";
import { ItemView } from "../features/chat/acp/TurnActivityItem";
import { AnswerActions } from "../features/chat/AnswerActions";
import {
  formatWorkedDuration,
  formatWorkingDuration,
} from "../features/chat/transcriptSummary";
import {
  getAcpChildTranscript,
  type ChildTranscript,
} from "../services/acpChildTranscript";
import { ArrowLeft } from "../ui/leanIcons";
import {
  liveAgent,
  projectTaskResources,
} from "./agentArtifactTabPresentation";
import {
  childRefresh,
  useChildEpoch,
  scheduleChildRefresh,
  childActivityLabel,
  choiceName,
  invocationOf,
  SubagentAvatar,
  SubagentList,
  subagentTimes,
  useNow,
} from "./SubagentResourceRows";

export { AgentTaskSubagentSummary } from "./AgentTaskSubagentSummary";

const MAX_CHILD_ACTIVITIES = 32;
const MAX_VISIBLE_CHILD_ITEMS = 256;
const MAX_STALE_RETRIES = 3;

type ChildRead = Extract<ChildTranscript, { turns: unknown }>;
type ReadState = {
  key: string;
  value: ChildTranscript | "error";
  /** The latest full read for this child, kept across later failures. */
  good?: ChildRead;
  stale: number;
};

/** One deduplicated run of child items without the spawn prompt bubble. */
function childRows(read: ChildRead | undefined) {
  const seen = new Set<string>();
  const rows: AcpItem[] = [];
  for (const [turnIndex, childTurn] of (read?.turns ?? []).entries())
    for (const [index, child] of childTurn.items.entries()) {
      const prompt =
        turnIndex === 0 &&
        index === 0 &&
        child.kind === "text" &&
        child.payload.role === "user";
      if (prompt || seen.has(child.id)) continue;
      seen.add(child.id);
      rows.push(child);
    }
  return rows;
}

export function SubagentSurface(props: {
  page: AcpChatPage;
  projectGeneration?: string;
  itemId: string;
  agentKey?: string;
}) {
  const list = useSubagentPaneStore((state) => state.list);
  return list ? (
    <section className="subagent-surface" aria-label="Subagents">
      <SubagentList page={props.page} />
    </section>
  ) : (
    <SubagentChild {...props} />
  );
}

function SubagentChild({
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
  // The child identity; the child projection epoch is its invalidation token.
  const identity = canRead
    ? [
        page.projectId,
        projectGeneration,
        page.chatId,
        turn.sessionId,
        turn.runtimeEpoch,
        itemId,
        agentKey,
      ].join("\u0000")
    : "";
  const epoch = useChildEpoch(page);
  const recent = agentKey
    ? projectTaskResources(page, page.chatId).recentAgents
    : [];
  const invocation =
    agentKey && !superseded ? invocationOf(page, itemId, agentKey) : null;
  const observed = invocation
    ? recent.find((agent) => agent.id === invocation)
    : null;
  const live = liveAgent(observed?.status);
  const [read, setRead] = useState<ReadState | null>(null);
  const sync = useRef(childRefresh());
  const current = read?.key === identity ? read : null;
  const good = current?.good;
  const settled =
    !!good &&
    !live &&
    good.turns.every((child) => child.status !== "inProgress");
  const staleRetry =
    current &&
    typeof current.value === "object" &&
    current.value.kind === "stale"
      ? Math.min(current.stale, MAX_STALE_RETRIES)
      : 0;
  useEffect(() => {
    if (
      epoch === null ||
      !canRead ||
      settled ||
      staleRetry >= MAX_STALE_RETRIES ||
      !sessionId ||
      !runtimeEpoch ||
      !projectGeneration ||
      !agentKey
    )
      return;
    const state = sync.current;
    const fresh = state.identity !== identity;
    state.identity = identity;
    // Throttled to about once a second (scheduleChildRefresh), never
    // beside another read. A
    // user config, goal or fork command that meets a read in flight waits
    // for it in Rust (RuntimeHost::native_rpc), so reads cannot starve it.
    const schedule = () => scheduleChildRefresh(state);
    state.send = () => {
      state.timer = undefined;
      if (state.inFlight) {
        state.again = true;
        return;
      }
      state.inFlight = true;
      state.startedAt = performance.now();
      void getAcpChildTranscript({
        projectId: page.projectId,
        projectGeneration,
        chatId: page.chatId,
        sessionId,
        runtimeEpoch,
        itemId,
        agentKey,
      })
        // Only malformed successful responses throw; request failures are typed.
        .catch(() => "error" as const)
        .then((value) => {
          state.inFlight = false;
          if (state.identity === identity)
            setRead((previous) => {
              const prior = previous?.key === identity ? previous : null;
              const stale = typeof value === "object" && value.kind === "stale";
              return {
                key: identity,
                value,
                // A failure never replaces the last full read.
                good:
                  typeof value === "object" && "turns" in value
                    ? value
                    : prior?.good,
                stale: stale ? (prior?.stale ?? 0) + 1 : 0,
              };
            });
          if (state.again) {
            state.again = false;
            if (state.identity === identity) schedule();
            else state.send();
          }
        });
    };
    if (fresh) state.send();
    else schedule();
    return () => {
      clearTimeout(state.timer);
      state.timer = undefined;
      state.send = () => undefined;
    };
  }, [
    identity,
    epoch,
    settled,
    staleRetry,
    canRead,
    page.projectId,
    page.chatId,
    projectGeneration,
    sessionId,
    runtimeEpoch,
    itemId,
    agentKey,
  ]);
  const now = useNow(live);
  if (!turn || item?.kind !== "collaboration") return null;

  const label =
    observed?.label ??
    (item.payload.receiverLabels.length === 1
      ? item.payload.receiverLabels[0].trim() || "Subagent"
      : item.payload.operation === "started"
        ? item.payload.label
        : "Subagent");
  const time = observed ? subagentTimes(page, recent).get(observed.id) : null;
  const duration = time
    ? live
      ? formatWorkingDuration(time.startedAt, Math.max(now, time.startedAt))
      : formatWorkedDuration(time.startedAt, time.updatedAt)
    : null;
  const spawn = good?.spawnModel;
  const model = spawn
    ? [
        choiceName(page, "model", spawn.model),
        spawn.reasoningEffort &&
          choiceName(page, "thought_level", spawn.reasoningEffort),
      ]
        .filter(Boolean)
        .join(" · ")
    : null;

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
  const state = good ?? (current ? current.value : "loading");
  const rows = childRows(good);
  const visible = rows.slice(0, MAX_VISIBLE_CHILD_ITEMS);
  const last =
    settled && !good?.retentionLimited && rows.length === visible.length
      ? rows.at(-1)
      : undefined;
  const answer =
    last?.kind === "text" &&
    last.payload.role === "assistant" &&
    last.payload.text;
  return (
    <section className="subagent-surface" aria-label="Subagent activity">
      <header>
        <div className="subagent-surface__bar">
          <button
            type="button"
            aria-label="Back to subagents"
            onClick={() => useSubagentPaneStore.setState({ list: true })}
          >
            <ArrowLeft size={14} aria-hidden="true" />
          </button>
          <SubagentAvatar agentKey={agentKey ?? itemId} />
          <h1>{label}</h1>
          {model ? (
            <span className="subagent-surface__model">{model}</span>
          ) : null}
        </div>
        {duration ? (
          <p
            className={`subagent-surface__duration${live ? " acp-live-shimmer" : ""}`}
          >
            {duration}
          </p>
        ) : null}
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
              {state.kind === "fromEarlierSession" ? (
                <p>From an earlier session</p>
              ) : null}
              {state.retentionLimited ? (
                <p>Some later child activity was not retained.</p>
              ) : null}
              {rows.length > visible.length ? (
                <p>
                  Showing the first {MAX_VISIBLE_CHILD_ITEMS} retained child
                  items.
                </p>
              ) : null}
              {visible.length === 0 ? (
                <p>No child activity was captured.</p>
              ) : (
                <div className="acp-turn__items" aria-label="Child transcript">
                  {visible.map((child) => (
                    <ItemView
                      key={child.id}
                      item={child}
                      interactive={false}
                      neutralTextRole={
                        child.kind === "text" && child.payload.role === "system"
                      }
                    />
                  ))}
                </div>
              )}
              {/* Codex s4/f0485: a settled child's answer keeps Copy only. */}
              {answer ? <AnswerActions text={answer} /> : null}
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
