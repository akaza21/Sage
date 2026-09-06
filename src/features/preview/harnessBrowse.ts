import type { PreviewProjectionOwner } from "../../domain/previewProjection";

export const MAX_HARNESS_BROWSE_URL_BYTES = 2048;
export const MAX_HARNESS_BROWSE_SESSIONS = 2;
export const MAX_HARNESS_BROWSE_HISTORY = 8;

export type HarnessBrowseUrl = {
  href: string;
};

export type HarnessBrowseIntent = {
  owner: HarnessBrowseOwner;
  url: string;
};

export type HarnessBrowseOwner = PreviewProjectionOwner;

export type HarnessBrowseSession = {
  owner: HarnessBrowseOwner;
  href: string;
  revision: number;
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
let owner: HarnessBrowseOwner | null = null;
let visibleChatIds = new Set<string>();
const sessions = new Map<string, HarnessBrowseSession>();
const histories = new Map<string, BrowseHistoryStack>();
const listeners = new Set<() => void>();

function emitBrowseSessions(): void {
  for (const listener of listeners) listener();
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
    new TextEncoder().encode(raw).byteLength > MAX_HARNESS_BROWSE_URL_BYTES ||
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
  if (
    text.length === 0 ||
    new TextEncoder().encode(text).byteLength > MAX_HARNESS_BROWSE_URL_BYTES
  ) {
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

function recordHistory(key: string, href: string): void {
  const existing = histories.get(key);
  if (!existing) {
    histories.set(key, { hrefs: [href], index: 0 });
    return;
  }
  if (existing.hrefs[existing.index] === href) return;
  const next = existing.hrefs.slice(0, existing.index + 1);
  next.push(href);
  const overflow = next.length - MAX_HARNESS_BROWSE_HISTORY;
  existing.hrefs = overflow > 0 ? next.slice(overflow) : next;
  existing.index = existing.hrefs.length - 1;
}

function ownerKey(owner: HarnessBrowseOwner | null | undefined): string | null {
  if (!owner) return null;
  return JSON.stringify([
    owner.projectId,
    owner.projectGeneration,
    owner.transitionEpoch,
    owner.chatId,
    owner.sessionId,
    owner.runtimeEpoch,
  ]);
}

function sameOwner(
  left: HarnessBrowseOwner | null | undefined,
  right: HarnessBrowseOwner | null | undefined,
): boolean {
  return Boolean(
    left &&
    right &&
    left.projectId === right.projectId &&
    left.projectGeneration === right.projectGeneration &&
    left.transitionEpoch === right.transitionEpoch &&
    left.chatId === right.chatId &&
    left.sessionId === right.sessionId &&
    left.runtimeEpoch === right.runtimeEpoch,
  );
}

function moveHistory(
  browseOwner: HarnessBrowseOwner,
  delta: -1 | 1,
): HarnessBrowseSession | null {
  const key = ownerKey(browseOwner);
  if (!key) return null;
  const existing = sessions.get(key);
  const stack = histories.get(key);
  const nextIndex = (stack?.index ?? 0) + delta;
  const href = stack?.hrefs[nextIndex];
  if (!existing || !stack || !href) return null;
  const admitted = admitHarnessBrowseUrl(href);
  if (!admitted) return null;
  stack.index = nextIndex;
  const session: HarnessBrowseSession = {
    owner: existing.owner,
    href: admitted.href,
    revision: nextRevision,
  };
  nextRevision += 1;
  sessions.set(key, session);
  emitBrowseSessions();
  return session;
}

function retainVisibleChats(): void {
  for (const [key, session] of sessions) {
    if (!visibleChatIds.has(session.owner.chatId)) {
      sessions.delete(key);
      histories.delete(key);
    }
  }
}

function retainOwnerScope(nextOwner: HarnessBrowseOwner): void {
  for (const [key, session] of sessions) {
    const current = session.owner;
    if (
      current.projectId !== nextOwner.projectId ||
      current.projectGeneration !== nextOwner.projectGeneration ||
      current.transitionEpoch !== nextOwner.transitionEpoch
    ) {
      sessions.delete(key);
      histories.delete(key);
    }
  }
}

export function applyBrowseIntent(
  intent: HarnessBrowseIntent,
): HarnessBrowseSession | null {
  const key = ownerKey(intent.owner);
  if (
    !key ||
    !sameOwner(intent.owner, owner) ||
    !visibleChatIds.has(intent.owner.chatId)
  ) {
    return null;
  }
  const admitted = admitHarnessBrowseUrl(intent.url);
  if (!admitted) return null;
  const existing = sessions.get(key);
  if (existing?.href === admitted.href) {
    recordHistory(key, admitted.href);
    return existing;
  }
  const session: HarnessBrowseSession = {
    owner: intent.owner,
    href: admitted.href,
    revision: nextRevision,
  };
  nextRevision += 1;
  const chatSessions = [...sessions.entries()].filter(
    ([, candidate]) => candidate.owner.chatId === intent.owner.chatId,
  );
  if (!existing && chatSessions.length >= MAX_HARNESS_BROWSE_SESSIONS) {
    let oldestKey: string | null = null;
    let oldestRevision = Number.MAX_SAFE_INTEGER;
    for (const [candidateKey, candidate] of chatSessions) {
      if (candidate.revision < oldestRevision) {
        oldestRevision = candidate.revision;
        oldestKey = candidateKey;
      }
    }
    if (oldestKey) {
      sessions.delete(oldestKey);
      histories.delete(oldestKey);
    }
  }
  recordHistory(key, admitted.href);
  sessions.set(key, session);
  emitBrowseSessions();
  return session;
}

export function browseHistoryForOwner(
  browseOwner: HarnessBrowseOwner,
): HarnessBrowseHistory {
  const key = ownerKey(browseOwner);
  return key ? historyFlags(histories.get(key)) : EMPTY_HISTORY;
}

export function browseBack(
  browseOwner: HarnessBrowseOwner,
): HarnessBrowseSession | null {
  return moveHistory(browseOwner, -1);
}

export function browseForward(
  browseOwner: HarnessBrowseOwner,
): HarnessBrowseSession | null {
  return moveHistory(browseOwner, 1);
}

export function setHarnessBrowseOwner(
  nextOwner: HarnessBrowseOwner | null,
  nextVisibleChatIds: readonly string[] = nextOwner ? [nextOwner.chatId] : [],
): void {
  const key = ownerKey(nextOwner);
  const next = key ? nextOwner : null;
  const visible = new Set(nextVisibleChatIds.slice(0, 2));
  if (!next || !visible.has(next.chatId)) visible.clear();
  const changed =
    !sameOwner(next, owner) ||
    visible.size !== visibleChatIds.size ||
    [...visible].some((id) => !visibleChatIds.has(id));
  if (!changed) return;
  if (next) retainOwnerScope(next);
  owner = next;
  visibleChatIds = visible;
  retainVisibleChats();
  emitBrowseSessions();
}

export function harnessBrowseOwner(): HarnessBrowseOwner | null {
  return owner;
}

export function browseSessionForOwner(
  browseOwner: HarnessBrowseOwner,
): HarnessBrowseSession | null {
  const key = ownerKey(browseOwner);
  return key ? (sessions.get(key) ?? null) : null;
}

export function subscribeBrowseSessions(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function clearBrowseSession(browseOwner: HarnessBrowseOwner): void {
  const key = ownerKey(browseOwner);
  if (!key) return;
  const hadHistory = histories.delete(key);
  if (!sessions.delete(key) && !hadHistory) return;
  emitBrowseSessions();
}

export function clearBrowseSessions(): void {
  const hadSessions = sessions.size > 0 || histories.size > 0;
  const hadOwner = owner !== null;
  sessions.clear();
  histories.clear();
  owner = null;
  visibleChatIds.clear();
  if (hadSessions || hadOwner) emitBrowseSessions();
}

export function listBrowseSessions(
  visibleChatIds: readonly string[] = [],
): HarnessBrowseSession[] {
  const visible = new Set(visibleChatIds.slice(0, 2));
  return [...sessions.values()]
    .filter(
      (session) => visible.size === 0 || visible.has(session.owner.chatId),
    )
    .sort((left, right) => left.revision - right.revision)
    .slice(0, 4);
}
