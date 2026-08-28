export const MAX_HARNESS_BROWSE_URL_BYTES = 2048;
export const MAX_HARNESS_BROWSE_SESSIONS = 2;
export const MAX_HARNESS_BROWSE_HISTORY = 8;

export type HarnessBrowseUrl = {
  href: string;
};

export type HarnessBrowseIntent = {
  chatId: string;
  url: string;
  sessionId?: string;
};

export type HarnessBrowseSession = {
  chatId: string;
  href: string;
  revision: number;
  sessionId: string | null;
};

export type HarnessBrowseHistory = {
  canBack: boolean;
  canForward: boolean;
};

type BrowseHistoryStack = {
  hrefs: string[];
  index: number;
};

const EMPTY_HISTORY: HarnessBrowseHistory = {
  canBack: false,
  canForward: false,
};

const URL_IN_TEXT = /https?:\/\/[^\s<>"'`]+/gu;

let nextRevision = 1;
let ownerChatId: string | null = null;
const sessions = new Map<string, HarnessBrowseSession>();
const histories = new Map<string, BrowseHistoryStack>();
const listeners = new Set<() => void>();

function emitBrowseSessions(): void {
  for (const listener of listeners) listener();
}

function isSafeChatId(value: string): boolean {
  return value.length > 0 && value.length <= 512 && !value.includes("\0");
}

function isLoopbackHost(hostname: string): boolean {
  return hostname === "127.0.0.1" || hostname === "[::1]" || hostname === "::1";
}

function hasForbiddenControlChars(raw: string): boolean {
  for (let index = 0; index < raw.length; index += 1) {
    const code = raw.charCodeAt(index);
    if (code < 32 || code === 127) return true;
  }
  return false;
}

export function admitHarnessBrowseUrl(raw: string): HarnessBrowseUrl | null {
  if (
    raw.length === 0 ||
    raw.length > MAX_HARNESS_BROWSE_URL_BYTES ||
    hasForbiddenControlChars(raw)
  ) {
    return null;
  }
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    return null;
  }
  if (
    (parsed.protocol !== "http:" && parsed.protocol !== "https:") ||
    parsed.username !== "" ||
    parsed.password !== "" ||
    parsed.port === "" ||
    !isLoopbackHost(parsed.hostname)
  ) {
    return null;
  }
  const host = parsed.hostname === "::1" ? "[::1]" : parsed.hostname;
  const path = `${parsed.pathname}${parsed.search}${parsed.hash}`;
  return { href: `${parsed.protocol}//${host}:${parsed.port}${path}` };
}

export function extractAdmittedBrowseUrl(
  text: string,
): HarnessBrowseUrl | null {
  if (text.length === 0 || text.length > MAX_HARNESS_BROWSE_URL_BYTES) {
    return null;
  }
  const matches = text.match(URL_IN_TEXT) ?? [];
  const scanLimit = Math.min(matches.length, 8);
  for (let index = 0; index < scanLimit; index += 1) {
    const candidate = matches[index];
    if (!candidate) continue;
    const admitted = admitHarnessBrowseUrl(candidate.replace(/[.,);]+$/u, ""));
    if (admitted) return admitted;
  }
  return null;
}

function historyFlags(
  stack: BrowseHistoryStack | undefined,
): HarnessBrowseHistory {
  if (!stack) return EMPTY_HISTORY;
  return {
    canBack: stack.index > 0,
    canForward: stack.index + 1 < stack.hrefs.length,
  };
}

function forgetHistory(chatId: string) {
  histories.delete(chatId);
}

function recordHistory(chatId: string, href: string) {
  const existing = histories.get(chatId);
  if (!existing) {
    histories.set(chatId, { hrefs: [href], index: 0 });
    return;
  }
  if (existing.hrefs[existing.index] === href) return;
  const next = existing.hrefs.slice(0, existing.index + 1);
  next.push(href);
  const overflow = next.length - MAX_HARNESS_BROWSE_HISTORY;
  existing.hrefs = overflow > 0 ? next.slice(overflow) : next;
  existing.index = existing.hrefs.length - 1;
}

function moveHistory(
  chatId: string,
  delta: -1 | 1,
): HarnessBrowseSession | null {
  const existing = sessions.get(chatId);
  const stack = histories.get(chatId);
  const nextIndex = (stack?.index ?? 0) + delta;
  const href = stack?.hrefs[nextIndex];
  if (!existing || !stack || !href) return null;
  const admitted = admitHarnessBrowseUrl(href);
  if (!admitted) return null;
  stack.index = nextIndex;
  const session: HarnessBrowseSession = {
    chatId,
    href: admitted.href,
    revision: nextRevision,
    sessionId: existing.sessionId,
  };
  nextRevision += 1;
  sessions.set(chatId, session);
  emitBrowseSessions();
  return session;
}

export function applyBrowseIntent(
  intent: HarnessBrowseIntent,
): HarnessBrowseSession | null {
  if (!isSafeChatId(intent.chatId)) return null;
  if (intent.sessionId && !isSafeChatId(intent.sessionId)) return null;
  const admitted = admitHarnessBrowseUrl(intent.url);
  if (!admitted) return null;
  const existing = sessions.get(intent.chatId);
  if (
    existing?.sessionId &&
    intent.sessionId &&
    existing.sessionId !== intent.sessionId
  ) {
    return null;
  }
  if (existing?.href === admitted.href) {
    recordHistory(intent.chatId, admitted.href);
    return existing;
  }
  const session: HarnessBrowseSession = {
    chatId: intent.chatId,
    href: admitted.href,
    revision: nextRevision,
    sessionId: intent.sessionId ?? existing?.sessionId ?? null,
  };
  nextRevision += 1;
  if (!existing && sessions.size >= MAX_HARNESS_BROWSE_SESSIONS) {
    let oldestId: string | null = null;
    let oldestRevision = Number.MAX_SAFE_INTEGER;
    for (const [chatId, candidate] of sessions) {
      if (candidate.revision < oldestRevision) {
        oldestRevision = candidate.revision;
        oldestId = chatId;
      }
    }
    if (oldestId) {
      sessions.delete(oldestId);
      forgetHistory(oldestId);
    }
  }
  recordHistory(intent.chatId, admitted.href);
  sessions.set(intent.chatId, session);
  emitBrowseSessions();
  return session;
}

export function browseHistoryForChat(chatId: string): HarnessBrowseHistory {
  return historyFlags(histories.get(chatId));
}

export function browseBack(chatId: string): HarnessBrowseSession | null {
  return moveHistory(chatId, -1);
}

export function browseForward(chatId: string): HarnessBrowseSession | null {
  return moveHistory(chatId, 1);
}

export function setHarnessBrowseOwner(chatId: string | null): void {
  const next = chatId && isSafeChatId(chatId) ? chatId : null;
  if (next === ownerChatId) return;
  ownerChatId = next;
  emitBrowseSessions();
}

export function harnessBrowseOwner(): string | null {
  return ownerChatId;
}

export function browseSessionForChat(
  chatId: string,
): HarnessBrowseSession | null {
  return sessions.get(chatId) ?? null;
}

export function subscribeBrowseSessions(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function clearBrowseSession(chatId: string): void {
  const hadHistory = histories.has(chatId);
  forgetHistory(chatId);
  if (!sessions.delete(chatId) && !hadHistory) return;
  emitBrowseSessions();
}

export function clearBrowseSessions(): void {
  const hadSessions = sessions.size > 0 || histories.size > 0;
  const hadOwner = ownerChatId !== null;
  sessions.clear();
  histories.clear();
  ownerChatId = null;
  if (hadSessions || hadOwner) emitBrowseSessions();
}

export function listBrowseSessions(): HarnessBrowseSession[] {
  return [...sessions.values()]
    .sort((left, right) => left.revision - right.revision)
    .slice(0, MAX_HARNESS_BROWSE_SESSIONS);
}
