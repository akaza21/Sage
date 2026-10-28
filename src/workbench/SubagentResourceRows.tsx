import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { useSubagentPaneStore } from "../app/subagentPaneStore";
import type { AcpChatPage, AcpItem } from "../domain/acpProjection";
import { avatarColor } from "../features/chat/acp/subagentActivity";
import { subscribeChildProjectionChanges } from "../services/acpChildTranscript";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { focusPopoverDialog } from "../ui/menuPopoverDialogFocus";
import {
  liveAgent,
  projectTaskResources,
} from "./agentArtifactTabPresentation";

type RecentAgent = ReturnType<
  typeof projectTaskResources
>["recentAgents"][number];
export type SubagentTime = { startedAt: number; updatedAt: number };

/** Latest child epoch; desktop reads wait for listener attachment to avoid gaps. Non-desktop reads never wait. */
// eslint-disable-next-line react-refresh/only-export-components -- SubagentChild's refresh state
export function useChildEpoch(page: AcpChatPage): number | null {
  const [notice, setNotice] = useState({ chat: "", epoch: 0, attached: false });
  const chat = page.projectId + "\u0000" + page.chatId;
  useEffect(() => {
    let live = true;
    let stop = () => {};
    const attached = () => {
      if (live)
        setNotice((previous) =>
          previous.chat === chat
            ? { ...previous, attached: true }
            : { chat, epoch: 0, attached: true },
        );
    };
    // Ignore notices from torn-down Chat listeners.
    subscribeChildProjectionChanges(page.projectId, page.chatId, (epoch) => {
      if (live) setNotice({ chat, epoch, attached: true });
    }).then(
      (unlisten) => {
        if (!live) return unlisten();
        stop = unlisten;
        attached();
      },
      // Without a listener, use page epochs.
      attached,
    );
    return () => {
      live = false;
      stop();
    };
  }, [chat, page.projectId, page.chatId]);
  const own = page.childProjectionEpoch ?? 0;
  const listenerless = !isTauriRuntime();
  if (notice.chat === chat && (notice.attached || listenerless))
    return Math.max(own, notice.epoch);
  return listenerless ? own : null;
}

/** A live child re-reads at most once per this window, and within it. */
const CHILD_REFRESH_MS = 1_000;

/** Single-flight, throttled reads for one open child. */
export type ChildRefresh = {
  identity: string;
  inFlight: boolean;
  /** The epoch moved while a read was in flight. */
  again: boolean;
  timer: ReturnType<typeof setTimeout> | undefined;
  /** When the latest read started (performance.now()). */
  startedAt: number;
  send: () => void;
};

// eslint-disable-next-line react-refresh/only-export-components -- SubagentChild's refresh state
export function childRefresh(): ChildRefresh {
  return {
    identity: "",
    inFlight: false,
    again: false,
    timer: undefined,
    startedAt: -Infinity,
    send: () => undefined,
  };
}

/** Leading/trailing throttle; never postpone a pending read during activity. */
// eslint-disable-next-line react-refresh/only-export-components -- SubagentChild's refresh state
export function scheduleChildRefresh(state: ChildRefresh) {
  if (state.timer !== undefined) return;
  const wait = state.startedAt + CHILD_REFRESH_MS - performance.now();
  if (wait <= 0) return state.send();
  state.timer = setTimeout(() => state.send(), wait);
}

export function childActivityLabel(item: AcpItem): string {
  if (item.kind === "toolCall") return item.payload.presentation.label;
  if (item.kind === "collaboration") return item.payload.label;
  if (item.kind === "thoughtSummary") return "Thought summary";
  if (item.kind === "plan") return "Plan";
  if (item.kind === "error") return item.payload.code;
  return item.kind === "text" ? "Message" : "Activity";
}

const statusText = {
  starting: "Starting",
  working: "Working",
  done: "Done",
  stopped: "Stopped",
  failed: "Failed",
} as const;

/** Re-renders each second only while something is live. */
export function useNow(live: boolean): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), live ? 1_000 : 60_000);
    return () => clearInterval(timer);
  }, [live]);
  return now;
}

export function choiceName(page: AcpChatPage, category: string, value: string) {
  for (const option of page.sessionConfig?.configOptions ?? [])
    if (option.type === "select" && (option.category ?? option.id) === category)
      for (const choice of option.options.flatMap((entry) =>
        "options" in entry ? entry.options : [entry],
      ))
        if (choice.value === value) return choice.name;
  return value;
}

/** Bind observations to the latest preceding spawn, not a later reused key. */
export function invocationOf(
  page: AcpChatPage,
  itemId: string,
  agentKey: string,
) {
  let spawn: string | null = null;
  for (const turn of page.turns)
    for (const item of turn.items) {
      if (
        item.kind === "collaboration" &&
        item.payload.operation === "spawnAgent" &&
        item.payload.agentKeys?.includes(agentKey)
      )
        spawn = item.id;
      if (item.id === itemId) return spawn && `${spawn}:${agentKey}`;
    }
  return null;
}

/** Per-invocation times; later events belong to the key's latest spawn. */
export function subagentTimes(
  page: AcpChatPage,
  agents: readonly RecentAgent[],
): Map<string, SubagentTime> {
  const times = new Map<string, SubagentTime>();
  const byItem = new Map(
    agents.map((agent) => [
      agent.agentKey ? agent.id.slice(0, -agent.agentKey.length - 1) : agent.id,
      agent,
    ]),
  );
  const latest = new Map<string, SubagentTime>();
  for (const turn of page.turns.slice(-128))
    for (const item of turn.items) {
      const agent = byItem.get(item.id);
      if (agent) {
        const time = { startedAt: item.createdAt, updatedAt: item.updatedAt };
        times.set(agent.id, time);
        if (agent.agentKey) latest.set(agent.agentKey, time);
      } else if (item.kind === "collaboration")
        for (const key of [
          item.payload.agentKey,
          ...(item.payload.agentKeys ?? []),
        ]) {
          const time = key ? latest.get(key) : undefined;
          if (time) time.updatedAt = Math.max(time.updatedAt, item.updatedAt);
        }
    }
  return times;
}

/** Active rows count seconds; settled rows read `just now`, then m/h/d. */
export function subagentAge(updatedAt: number, now: number, active: boolean) {
  const seconds = Math.max(0, Math.floor((now - updatedAt) / 1_000));
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  return seconds < 60
    ? active
      ? `${seconds}s`
      : "just now"
    : hours >= 24
      ? `${Math.floor(hours / 24)}d`
      : hours
        ? `${hours}h`
        : `${minutes}m`;
}

export function SubagentAvatar({ agentKey }: { agentKey: string }) {
  return (
    <span
      className={`acp-subagent-row__glyph acp-subagent-row__glyph--${avatarColor(agentKey)} subagent-avatar`}
      aria-hidden="true"
    />
  );
}

export function SubagentList({ page }: { page: AcpChatPage }) {
  const agents = projectTaskResources(page, page.chatId).recentAgents;
  const times = subagentTimes(page, agents);
  const active = agents.filter((agent) => liveAgent(agent.status));
  const done = agents.filter((agent) => !liveAgent(agent.status));
  const now = useNow(active.length > 0);
  const section = (title: string, rows: RecentAgent[], live: boolean) =>
    rows.length ? (
      <section aria-label={`${title} subagents`}>
        <h2>
          {title} · {rows.length}
        </h2>
        {rows.map((agent, index) => {
          const time = times.get(agent.id);
          const status =
            agent.status === "done" ? null : statusText[agent.status];
          const content = (
            <>
              <SubagentAvatar agentKey={agent.agentKey ?? agent.id} />
              <span className="subagent-list__text">
                <span>{agent.label}</span>
                {status ? (
                  <span
                    className={
                      agent.status === "working"
                        ? "acp-live-shimmer"
                        : undefined
                    }
                  >
                    {status}
                  </span>
                ) : null}
              </span>
              {time ? (
                <time>{subagentAge(time.updatedAt, now, live)}</time>
              ) : null}
            </>
          );
          const target = agent.target;
          return target ? (
            <button
              key={agent.id}
              type="button"
              className="subagent-list__row"
              aria-label={`Open subagent ${agent.label === "Subagent" ? `Subagent ${index + 1}` : agent.label}`}
              onClick={() => useSubagentPaneStore.getState().show(target)}
            >
              {content}
            </button>
          ) : (
            <p key={agent.id} className="subagent-list__row">
              {content}
            </p>
          );
        })}
      </section>
    ) : null;
  return (
    <div className="subagent-list">
      {section("Active", active, true)}
      {section("Done", done, false)}
      {agents.length ? null : <p>No subagents in recent turns.</p>}
    </div>
  );
}

export function SubagentResourceRows({
  agents,
  onSelect,
}: {
  agents: readonly RecentAgent[];
  onSelect: () => void;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const dialog = listRef.current?.closest<HTMLElement>('[role="dialog"]');
    if (dialog && dialog === document.activeElement) {
      focusPopoverDialog(dialog);
    }
  }, []);
  return (
    <div ref={listRef} className="agent-task-header__agent-list">
      {agents.map((agent, index) => {
        const target = agent.target;
        const Row = target ? "button" : "p";
        return (
          <Row
            className="agent-task-header__agent agent-task-header__resource-row"
            key={agent.id}
            {...(target
              ? {
                  type: "button" as const,
                  "aria-label": `Open subagent ${agent.label === "Subagent" ? `Subagent ${index + 1}, ${agent.status}` : agent.label}`,
                  onClick: () => {
                    useSubagentPaneStore.getState().show(target);
                    onSelect();
                  },
                }
              : {})}
          >
            <span>{agent.label}</span>
            <span>{statusText[agent.status]}</span>
          </Row>
        );
      })}
    </div>
  );
}
