// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
import { chatAttentionStore } from "../../../src/app/chatAttentionStore";
import type { AcpChatPage } from "../../../src/domain/acp";
import type {
  ChatAttentionEvent,
  ChatAttentionV1,
} from "../../../src/domain/chatAttention";
import { attention } from "../../../src/domain/chatAttention.fixture";
import { terminalTurnStates } from "../../../src/features/chat/acp/statusSemantics";
import { chatAttention } from "../../../src/services/chatAttention";
import type { FixtureState } from "./agentTurnJourneyModel";

const values = new Map<string, ChatAttentionV1>();
const identities = new Map<string, string>();
const terminals = new Map<string, string>();
let listener: ((event: ChatAttentionEvent) => Promise<void>) | undefined;

export function publishJourneyAttention(page: AcpChatPage) {
  const previous = values.get(page.chatId);
  if (!previous) throw new Error("Journey attention ownership mismatch");
  const session = page.sessions.find(
    (session) => session.id === page.currentSessionId,
  );
  const turn = page.turns.at(-1);
  const terminal = [...page.turns]
    .reverse()
    .find((turn) => terminalTurnStates.has(turn.state));
  const approvals = page.turns
    .flatMap((turn) => turn.items)
    .filter(
      (item) =>
        item.kind === "permissionRequest" &&
        (item.payload.state === "received" || item.payload.state === "asked"),
    );
  const identity = JSON.stringify([
    page.currentSessionId,
    session?.state,
    turn?.state,
    terminal?.id,
    terminal?.state,
    approvals.map((item) => item.id),
  ]);
  if (identities.get(page.chatId) === identity) return;
  identities.set(page.chatId, identity);
  const terminalIdentity = terminal ? `${terminal.id}:${terminal.state}` : "";
  const terminalChanged = terminals.get(page.chatId) !== terminalIdentity;
  terminals.set(page.chatId, terminalIdentity);
  const activity = String(BigInt(previous.activityPosition) + 1n);
  const next = {
    ...previous,
    sessionId: page.currentSessionId,
    sessionState: session?.state ?? null,
    turnState: turn?.state ?? null,
    running: Boolean(turn && !terminalTurnStates.has(turn.state)),
    pendingApprovals: approvals.length,
    activityPosition: activity,
    revision: String(BigInt(previous.revision) + 1n),
    terminalPosition:
      terminal && terminalChanged ? activity : previous.terminalPosition,
    terminalState: terminal?.state ?? previous.terminalState,
  };
  values.set(page.chatId, next);
  void listener?.({
    projectId: page.projectId,
    chatId: page.chatId,
    revision: next.revision,
  });
}

export function installJourneyAttention(state: FixtureState) {
  const original = { ...chatAttention };
  values.clear();
  identities.clear();
  terminals.clear();
  for (const page of state.pages.values()) {
    values.set(
      page.chatId,
      attention({
        projectId: page.projectId,
        chatId: page.chatId,
        activityPosition: "0",
        revision: "0",
        turnState: null,
        terminalState: null,
        terminalPosition: null,
      }),
    );
    publishJourneyAttention(page);
  }
  const owned = (project: string, chat: string) => {
    const row = values.get(chat);
    if (!row || row.projectId !== project)
      throw new Error("Journey attention ownership mismatch");
    return row;
  };
  chatAttention.get = async (project, ids) => {
    if (ids.length > 64) throw new Error("Journey attention ID bound");
    return ids.map((chat) => ({ ...owned(project, chat) }));
  };
  chatAttention.ack = async (project, chat, observed) => {
    const row = owned(project, chat);
    if (BigInt(observed) > BigInt(row.activityPosition))
      throw new Error("Future journey read");
    if (
      row.readPosition === null ||
      BigInt(observed) > BigInt(row.readPosition)
    ) {
      row.readPosition = observed;
      row.revision = String(BigInt(row.revision) + 1n);
    }
    return { ...row };
  };
  chatAttention.markUnread = async (project, chat, expected) => {
    const row = owned(project, chat);
    if (row.revision !== expected) throw new Error("Stale journey unread");
    row.activityPosition = String(BigInt(row.activityPosition) + 1n);
    row.forcedUnreadPosition = row.activityPosition;
    row.revision = String(BigInt(row.revision) + 1n);
    return { ...row };
  };
  chatAttention.subscribe = async (callback) => {
    listener = callback;
    return () => {
      listener = undefined;
    };
  };
  const projects = [
    ...new Set([...values.values()].map((row) => row.projectId)),
  ];
  void chatAttentionStore.connect(
    projects.map((projectId) => ({
      projectId,
      chatIds: [...values.values()]
        .filter((row) => row.projectId === projectId)
        .map((row) => row.chatId),
    })),
  );
  return () => {
    chatAttentionStore.disconnect();
    listener = undefined;
    values.clear();
    identities.clear();
    terminals.clear();
    Object.assign(chatAttention, original);
  };
}
