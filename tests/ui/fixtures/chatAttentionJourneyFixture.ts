import {
  acpProjectionStoreFor,
  occupiedAcpProjectionSlotCount,
} from "../../../src/app/acpProjectionRegistry";
import { chatAttentionStore } from "../../../src/app/chatAttentionStore";
import {
  attentionUnread,
  type ChatAttentionEvent,
  type ChatAttentionV1,
} from "../../../src/domain/chatAttention";
import { attention } from "../../../src/domain/chatAttention.fixture";
import { acpDesktop } from "../../../src/services/acpDesktop";
import {
  chatAttention,
  withPresentedAttention,
} from "../../../src/services/chatAttention";
import {
  installAgentTurnJourneyFixture,
  runJourneyEvent,
} from "./agentTurnJourneyFixture";
import { JOURNEY_IDS, type JourneyChat } from "./agentTurnJourneyModel";

const STORAGE_KEY = "sage.test.attention-service.v1";
const chats = ["alpha", "beta", "gamma"] as const;
let rows: Record<JourneyChat, ChatAttentionV1>;
const fetches: string[] = [];
let listener: ((event: ChatAttentionEvent) => Promise<void>) | undefined;

function persist() {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(rows));
}

function owned(project: string, chat: string): ChatAttentionV1 {
  const row = Object.values(rows).find(
    (row) => row.projectId === project && row.chatId === chat,
  );
  if (!row) throw new Error("Attention fixture ownership mismatch");
  return row;
}

export async function installAttentionJourneyFixture(reset: boolean) {
  await installAgentTurnJourneyFixture();
  for (const chat of chats)
    await runJourneyEvent({ type: "seedHistory", chat, turns: 1 });
  const saved = reset ? null : localStorage.getItem(STORAGE_KEY);
  rows = saved
    ? JSON.parse(saved)
    : Object.fromEntries(
        chats.map((chat) => [
          chat,
          attention({
            projectId:
              chat === "gamma" ? JOURNEY_IDS.projectB : JOURNEY_IDS.projectA,
            chatId: JOURNEY_IDS[chat],
          }),
        ]),
      );
  if (Object.keys(rows).length !== 3)
    throw new Error("Attention fixture must have three rows");
  fetches.length = 0;
  chatAttention.get = async (project, ids) => {
    if (ids.length > 64) throw new Error("Attention fixture ID bound");
    return ids.map((chat) => ({ ...owned(project, chat) }));
  };
  chatAttention.subscribe = async (callback) => {
    listener = callback;
    return () => {
      listener = undefined;
    };
  };
  chatAttention.ack = async (project, chat, observed) => {
    const row = owned(project, chat);
    if (BigInt(observed) > BigInt(row.activityPosition))
      throw new Error("Future fixture read");
    if (
      row.readPosition === null ||
      BigInt(observed) > BigInt(row.readPosition)
    ) {
      row.readPosition = observed;
      row.revision = String(BigInt(row.revision) + 1n);
      persist();
    }
    return { ...row };
  };
  chatAttention.markUnread = async (project, chat, expected) => {
    const row = owned(project, chat);
    if (row.revision !== expected) throw new Error("Stale fixture unread");
    row.activityPosition = String(BigInt(row.activityPosition) + 1n);
    row.forcedUnreadPosition = row.activityPosition;
    row.revision = String(BigInt(row.revision) + 1n);
    persist();
    return { ...row };
  };
  const getPage = acpDesktop.getChatPage;
  acpDesktop.getChatPage = async (request) => {
    if (fetches.length >= 64)
      throw new Error("Attention fixture transcript fetch bound");
    fetches.push(request.chatId);
    const detail = await getPage(request);
    if (request.cursor) return detail;
    // Native returns the observed position from the same snapshot as the
    // attention summary; here this fixture's rows are that summary.
    const row = owned(request.projectId, request.chatId);
    return withPresentedAttention({ ...detail }, row.activityPosition);
  };
  await chatAttentionStore.connect([
    {
      projectId: JOURNEY_IDS.projectA,
      chatIds: [JOURNEY_IDS.alpha, JOURNEY_IDS.beta],
    },
    { projectId: JOURNEY_IDS.projectB, chatIds: [JOURNEY_IDS.gamma] },
  ]);
  acpProjectionStoreFor(JOURNEY_IDS.alpha).getState().requestReload();
  persist();
}

export async function publishAttentionJourney(
  chat: JourneyChat,
  state: "waitingApproval" | "failed" | "completed",
) {
  const row = rows[chat];
  row.activityPosition = String(BigInt(row.activityPosition) + 1n);
  row.revision = String(BigInt(row.revision) + 1n);
  row.turnState = state;
  row.running = state === "waitingApproval";
  row.pendingApprovals = state === "waitingApproval" ? 1 : 0;
  if (state !== "waitingApproval") {
    row.terminalState = state;
    row.terminalPosition = row.activityPosition;
  }
  persist();
  await runJourneyEvent({ type: "publish", chat, state });
  await listener?.({
    projectId: row.projectId,
    chatId: row.chatId,
    revision: row.revision,
  });
}

export function attentionJourneyAudit() {
  return {
    rows: structuredClone(rows),
    fetches: [...fetches],
    unread: Object.fromEntries(
      chats.map((chat) => [chat, attentionUnread(rows[chat], false)]),
    ),
    transcriptSlots: occupiedAcpProjectionSlotCount(),
  };
}
