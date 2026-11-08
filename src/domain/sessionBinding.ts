/**
 * The session binding decisions Rust projects for a Chat: a harness build
 * change a session continued across, or the refusal that ended a session
 * whose recorded build cannot continue on the installed one.
 */
import type { AcpChatPage, RuntimeEpoch, TurnProjection } from "./acp";
import {
  buildChatHandoffDraft,
  MAX_HANDOFF_CONTEXT_BYTES,
  MAX_HANDOFF_CONTEXT_MESSAGES,
  MAX_HANDOFF_MESSAGE_BYTES,
  type ChatHandoffContextEntry,
} from "./chatHandoff";
import { isRecord } from "./guards";
import { truncateUtf8, utf8ByteLength } from "./text";

/** Rust bounds each recorded harness version to 128 bytes. */
const VERSION_BYTES_MAX = 128;
/** The Chat Handoff source-label limit. */
const MAX_SOURCE_LABEL_BYTES = 1024;

export type SessionBindingNotice =
  | {
      kind: "continued";
      runtimeEpoch: RuntimeEpoch;
      change: "nodeOnly" | "compatibleAdapter";
      fromVersion: string | null;
      toVersion: string;
    }
  | {
      kind: "refused";
      reason: "unknownBuild" | "otherFamily";
      fromVersion: string | null;
      toVersion: string;
    };

export type RefusedSessionBinding = Extract<
  SessionBindingNotice,
  { kind: "refused" }
>;

function version(value: unknown): string | null | undefined {
  if (value === null) return null;
  return typeof value === "string" &&
    value.length > 0 &&
    utf8ByteLength(value) <= VERSION_BYTES_MAX
    ? value
    : undefined;
}

/** Returns `undefined` for a malformed notice so the session is rejected. */
export function decodeSessionBindingNotice(
  value: unknown,
): SessionBindingNotice | undefined {
  if (!isRecord(value)) return undefined;
  const fromVersion = version(value.fromVersion);
  const toVersion = version(value.toVersion);
  if (fromVersion === undefined || !toVersion) return undefined;
  if (
    value.kind === "continued" &&
    (value.change === "nodeOnly" || value.change === "compatibleAdapter") &&
    typeof value.runtimeEpoch === "string" &&
    /^[1-9][0-9]{0,18}$/.test(value.runtimeEpoch)
  ) {
    return {
      kind: "continued",
      runtimeEpoch: value.runtimeEpoch as RuntimeEpoch,
      change: value.change,
      fromVersion,
      toVersion,
    };
  }
  if (
    value.kind === "refused" &&
    (value.reason === "unknownBuild" || value.reason === "otherFamily")
  ) {
    return { kind: "refused", reason: value.reason, fromVersion, toVersion };
  }
  return undefined;
}

/** The one-line notice shown above the first Turn after a drift. */
export function continuedNoticeText(
  notice: Extract<SessionBindingNotice, { kind: "continued" }>,
): string {
  if (notice.change === "nodeOnly") {
    return "Node.js was updated. This chat continues in the same session.";
  }
  const from = notice.fromVersion ?? "an earlier build";
  return `The agent adapter was updated (${from} → ${notice.toVersion}). This chat continues in the same session.`;
}

export function refusedNoticeText(notice: RefusedSessionBinding): string {
  const from = notice.fromVersion ?? "an earlier build";
  return notice.reason === "unknownBuild"
    ? `Sage hasn't verified the installed adapter (${notice.toVersion}) for resuming sessions started on ${from}. Your chat history is kept.`
    : `The installed adapter (${notice.toVersion}) uses a different session protocol than ${from}. Your chat history is kept.`;
}

/**
 * The refusal to offer recovery for: the Chat has no current session and
 * its latest session ended because its build cannot continue.
 */
export function refusedSessionBinding(
  page: AcpChatPage,
): RefusedSessionBinding | null {
  if (page.currentSessionId) return null;
  const latest = page.sessions.reduce<AcpChatPage["sessions"][number] | null>(
    (current, session) =>
      !current || session.ordinal > current.ordinal ? session : current,
    null,
  );
  const notice = latest?.bindingNotice;
  return notice?.kind === "refused" ? notice : null;
}

const continuedNotices = new WeakMap<AcpChatPage, Map<string, string>>();

/**
 * The drift notice to show before `turn`: only before the first loaded Turn
 * of the runtime epoch that crossed the drift, so each drift shows once.
 */
export function continuedNoticeBefore(
  page: AcpChatPage,
  turn: TurnProjection,
): string | null {
  let byTurn = continuedNotices.get(page);
  if (!byTurn) {
    byTurn = new Map();
    for (const session of page.sessions) {
      const notice = session.bindingNotice;
      if (notice?.kind !== "continued") continue;
      const first = page.turns.find(
        (candidate) =>
          candidate.sessionId === session.id &&
          candidate.runtimeEpoch === notice.runtimeEpoch,
      );
      if (first) byTurn.set(first.id, continuedNoticeText(notice));
    }
    continuedNotices.set(page, byTurn);
  }
  return byTurn.get(turn.id) ?? null;
}

/**
 * A bounded, editable summary of the visible conversation for the first
 * prompt of the new session: the newest text messages that fit the Chat
 * Handoff limits, each clipped to the per-message limit.
 */
export function sessionContinuationDraft(
  chatTitle: string,
  page: AcpChatPage,
): string {
  const entries: ChatHandoffContextEntry[] = page.turns.flatMap((turn) =>
    turn.items.flatMap((item) =>
      item.kind === "text" && item.origin === "primary"
        ? [{ role: item.payload.role, text: item.payload.text }]
        : [],
    ),
  );
  const selected: ChatHandoffContextEntry[] = [];
  let bytes = 0;
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    if (selected.length === MAX_HANDOFF_CONTEXT_MESSAGES) break;
    const text = truncateUtf8(entries[index].text, MAX_HANDOFF_MESSAGE_BYTES);
    const size = utf8ByteLength(text);
    if (bytes + size > MAX_HANDOFF_CONTEXT_BYTES) break;
    bytes += size;
    selected.unshift({ role: entries[index].role, text });
  }
  return buildChatHandoffDraft(
    truncateUtf8(chatTitle.trim() || "Chat", MAX_SOURCE_LABEL_BYTES),
    selected,
  );
}
