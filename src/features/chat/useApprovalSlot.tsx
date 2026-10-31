import {
  Component,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";

import type {
  AcpItem,
  PermissionDecision,
  TurnProjection,
} from "../../domain/acp";
import { acpDesktop } from "../../services/acpDesktop";
import { awaitingPermission } from "./acp/statusSemantics";
import { ApprovalPart } from "./lazyParts";

type PermissionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

/** A keypress this recent means the person is typing; never steal focus. */
export const TYPING_HOLD_MS = 500;
let lastKeyAt = Number.NEGATIVE_INFINITY;
let lastFocused: Element | null = null;
let tracking = false;

/** One capture-phase tracker for every pane: cheap, and installed before any card. */
function trackTyping() {
  if (tracking || typeof document === "undefined") return;
  tracking = true;
  document.addEventListener("keydown", () => (lastKeyAt = Date.now()), true);
  document.addEventListener("focusin", (event) => {
    lastFocused = event.target instanceof Element ? event.target : null;
  });
}

const typesInto = (element: Element) =>
  element.matches("input, textarea, select, iframe") ||
  (element as HTMLElement).isContentEditable;

/**
 * Whether the approval card of `pane` may move focus: only from the body or
 * from this pane's own transcript or card. Never from an editable field (the
 * composer, find bar, a rename input), an iframe (the browser preview), or
 * anywhere outside this pane. A focus that just fell to the body counts as
 * the element it fell from while that element is still in the document.
 */
export function approvalFocusInReach(pane: Element | null) {
  const active = document.activeElement;
  const fellFrom = lastFocused && lastFocused.isConnected ? lastFocused : null;
  const from =
    !active || active === document.body || active === document.documentElement
      ? fellFrom
      : active;
  if (!from || from === document.body) return true;
  if (typesInto(from) || !pane?.contains(from)) return false;
  return Boolean(from.closest(".message-list, .acp-permission-prompt"));
}

/** Focus is in reach and nobody pressed a key within the typing hold. */
export function approvalFocusAllowed(composer: HTMLElement | null) {
  return (
    Date.now() - lastKeyAt >= TYPING_HOLD_MS &&
    approvalFocusInReach(composer?.closest(".chat-surface") ?? null)
  );
}

/**
 * The decision hold shared by the loading fallback and the card: the time of
 * the last decision, and each fallback Deny (by item id) so the loaded card
 * shows it in flight and never focuses Allow for that request. Bounded.
 */
export const approvalHold = {
  at: Number.NEGATIVE_INFINITY,
  denies: new Map<string, Promise<unknown>>(),
};

/** Test seam: forget every recorded decision. */
export function clearApprovalDecisions() {
  approvalHold.at = Number.NEGATIVE_INFINITY;
  approvalHold.denies.clear();
}

export function resolveApproval(
  item: PermissionItem,
  decision: PermissionDecision = "deny",
) {
  return acpDesktop.resolvePermissionDecision({
    projectId: item.projectId,
    chatId: item.chatId,
    sessionId: item.sessionId,
    turnId: item.turnId,
    runtimeEpoch: item.runtimeEpoch,
    requestId: item.payload.requestId,
    decision,
  });
}

/** Shown while the card's chunk loads: Deny and Stop are always reachable. */
function ApprovalFallback({
  item,
  onCancelTurn,
}: {
  item: PermissionItem;
  onCancelTurn?: () => void;
}) {
  const [state, setState] = useState<"idle" | "sending" | "failed">("idle");
  return (
    <section
      className="acp-permission-prompt"
      role="group"
      aria-label="Permission request"
      aria-busy="true"
    >
      <p>Loading the approval…</p>
      {state === "failed" ? (
        <p className="acp-permission__error" role="alert">
          The denial could not be sent. Try again, or stop the Turn.
        </p>
      ) : null}
      {onCancelTurn ? (
        <button type="button" aria-label="Stop turn" onClick={onCancelTurn}>
          Stop
        </button>
      ) : null}
      <button
        type="button"
        disabled={state === "sending"}
        onClick={() => {
          if (state === "sending") return;
          setState("sending");
          approvalHold.at = Date.now();
          const sent = resolveApproval(item);
          const { denies } = approvalHold;
          denies.set(item.id, sent);
          if (denies.size > 32) denies.delete(denies.keys().next().value!);
          sent.catch(() => setState("failed"));
        }}
      >
        Deny
      </button>
    </section>
  );
}

/** A chunk that fails to load hands the slot back to the composer. */
class ApprovalBoundary extends Component<
  { onError: () => void; children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch() {
    this.props.onError();
  }
  render() {
    return this.state.failed ? null : this.props.children;
  }
}

/**
 * A pending approval in the live turn takes the composer slot. The composer
 * stays mounted (hidden) so its draft survives. A card that cannot load restores the
 * composer, whose Stop control still ends the Turn.
 */
export function useApprovalSlot(
  turn: TurnProjection | null | undefined,
  composerRef: RefObject<HTMLTextAreaElement | null>,
  {
    focused = true,
    onCancelTurn,
  }: { focused?: boolean; onCancelTurn?: () => void } = {},
): { awaiting: boolean; slot: ReactNode } {
  trackTyping();
  const pending = turn?.items.filter(awaitingPermission) ?? [];
  const first = pending[0];
  const awaiting = Boolean(first);
  const [failedFor, setFailedFor] = useState<string | null>(null);
  const failed = Boolean(first) && failedFor === first.id;
  const wasAwaiting = useRef(false);
  const canTakeFocus = useCallback(
    () => approvalFocusAllowed(composerRef.current),
    [composerRef],
  );
  useEffect(
    () => () => {
      // A pane that unmounts never holds the tracker to its composer.
      if (lastFocused && lastFocused === composerRef.current)
        lastFocused = null;
    },
    [composerRef],
  );
  const holdsSlot = awaiting && !failed;
  useEffect(() => {
    // The unmounted prompt dropped focus to the body; hand it back to the
    // returning composer unless the person already moved focus elsewhere.
    if (
      wasAwaiting.current &&
      !holdsSlot &&
      document.activeElement === document.body
    )
      composerRef.current?.focus();
    wasAwaiting.current = holdsSlot;
  }, [holdsSlot, composerRef]);
  return {
    awaiting: holdsSlot,
    slot: !first ? null : failed ? (
      <p className="composer__notice" role="alert">
        The approval could not load. Stop the Turn, or reopen the Chat to try
        again.
      </p>
    ) : (
      <ApprovalBoundary key={first.id} onError={() => setFailedFor(first.id)}>
        <ApprovalPart
          part="PermissionPrompt"
          fallback={
            <ApprovalFallback item={first} onCancelTurn={onCancelTurn} />
          }
          items={pending}
          focused={focused}
          canTakeFocus={canTakeFocus}
          onCancelTurn={onCancelTurn}
        />
      </ApprovalBoundary>
    ),
  };
}
