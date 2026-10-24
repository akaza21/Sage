import { useEffect, useId, useRef, useState } from "react";
import { ChevronDown } from "../../../ui/leanIcons";

import type { AcpItem, PermissionDecision } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { MenuPopover } from "../../../ui/MenuPopover";
import { approvalFocusInReach, approvalHold } from "../useApprovalSlot";
import { ToolKindIcon } from "./ToolItem";
import "./permission-prompt.css";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

/** Header noun per gated tool kind; other kinds stay generic. */
const nouns: Record<string, string> = {
  execute: "Terminal",
  edit: "Edit",
  mcp: "MCP",
};

/** A follow-up request this soon after a decision never takes focus. */
const QUEUED_FOCUS_HOLD_MS = 300;

/**
 * Allow choices exactly as the harness offered them, in its order. A browser
 * permission's choices are Sage's own browser policy, never harness options.
 */
function permissionAllowChoices(item: PermissionProjectionItem) {
  const choices: [PermissionDecision, string][] = [];
  const { source, offeredOptionKinds, category } = item.payload;
  for (const kind of source === "browser"
    ? ["allow_once", "origin"]
    : new Set(offeredOptionKinds))
    if (kind === "allow_once") choices.push(["allowOnce", "Allow once"]);
    else if (kind === "origin")
      choices.push(["allowForOriginSession", "Allow for this tab and origin"]);
    else if (kind === "allow_always")
      choices.push([
        "allowAlways",
        category === "execute" ? "Allow similar commands" : "Always allow",
      ]);
  return choices;
}

/**
 * The pending approval, shown in the composer slot. The oldest request comes
 * first. Each button answers only for itself (Enter on Deny denies); Esc
 * denies from inside the card. Keys that began outside the card (a held
 * Enter from the composer, a stray keyup) are swallowed. Focus moves here
 * only for the focused pane when the person is not typing elsewhere.
 */
export function PermissionPrompt({
  items,
  focused = true,
  canTakeFocus = () => true,
  onCancelTurn,
}: {
  items: readonly PermissionProjectionItem[];
  /** Only the focused pane may take focus. */
  focused?: boolean;
  /** False while the composer is active or the person typed in the last 500 ms. */
  canTakeFocus?: () => boolean;
  /** Stop the Turn; always offered so the pane can never be stranded. */
  onCancelTurn?: () => void;
}) {
  const item = items[0];
  const menuId = useId();
  const primaryRef = useRef<HTMLButtonElement>(null);
  const firstRef = useRef<HTMLButtonElement>(null);
  const chevronRef = useRef<HTMLButtonElement>(null);
  const pressed = useRef(new Set<string>());
  const sectionRef = useRef<HTMLElement>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  /** The request in flight, or the one whose decision failed (negated). */
  const [sent, setSent] = useState<[string, boolean] | null>(null);
  const execute = item?.payload.category === "execute";
  const question = execute ? "Allow this command?" : "Allow this action?";
  const decide = useRef({ focused, canTakeFocus, question });
  useEffect(() => {
    decide.current = { focused, canTakeFocus, question };
  });
  useEffect(() => {
    if (!item) return;
    const { focused, canTakeFocus, question } = decide.current;
    // A Deny pressed on the loading fallback carries into the card.
    const denied = approvalHold.denies.get(item.id);
    denied?.catch(() => setSent([item.id, false]));
    const held = denied || Date.now() - approvalHold.at < QUEUED_FOCUS_HOLD_MS;
    const card = sectionRef.current;
    const pane = card?.closest(".chat-surface") ?? null;
    const inCard = card?.contains(document.activeElement);
    if (
      focused &&
      !held &&
      canTakeFocus() &&
      (inCard || approvalFocusInReach(pane))
    ) {
      (primaryRef.current ?? firstRef.current)?.focus();
      setAnnouncement("");
    } else {
      // A focused button of the previous request must not answer this one:
      // park focus on the card itself, where Enter does nothing.
      if (inCard) card?.focus();
      setAnnouncement(`Approval needed: ${question}`);
    }
    // Focus is decided once per request; later renders never steal it.
  }, [item?.id]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!item) return null;
  const { payload } = item;
  const browser = payload.source === "browser";
  const choices = permissionAllowChoices(item);
  const primary =
    choices.find(([decision]) => decision === "allowOnce") ?? choices[0];
  const offered = payload.offeredOptionKinds;
  const canDeny =
    browser ||
    !offered ||
    offered.includes("reject_once") ||
    offered.includes("reject_always");
  // A fallback Deny for this request is in flight until it settles here.
  const pending =
    sent?.[0] === item.id ? sent[1] : approvalHold.denies.has(item.id);
  const resolve = (decision: PermissionDecision) => {
    setMenuOpen(false);
    if (pending) return;
    approvalHold.at = Date.now();
    setSent([item.id, true]);
    void acpDesktop
      .resolvePermissionDecision({
        projectId: item.projectId,
        chatId: item.chatId,
        sessionId: item.sessionId,
        turnId: item.turnId,
        runtimeEpoch: item.runtimeEpoch,
        requestId: payload.requestId,
        decision,
      })
      .catch(() => setSent([item.id, false]));
  };
  const Detail = execute ? "pre" : "p";
  return (
    <section
      ref={sectionRef}
      tabIndex={-1}
      className="acp-permission-prompt"
      role="group"
      aria-label={browser ? "Browser permission" : "Permission request"}
      data-acp-item-id={item.id}
      onKeyDownCapture={(event) => {
        // Auto-repeat belongs to a key that went down before this card.
        if (event.repeat) {
          event.preventDefault();
          event.stopPropagation();
          return;
        }
        pressed.current.add(event.key);
      }}
      onKeyUpCapture={(event) => {
        // A keyup without its keydown here (Space from the composer) is inert.
        if (!pressed.current.delete(event.key)) {
          event.preventDefault();
          event.stopPropagation();
        }
      }}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || menuOpen || !canDeny) return;
        event.preventDefault();
        resolve("deny");
      }}
    >
      <header className="acp-permission-prompt__kind">
        <ToolKindIcon
          kind={browser ? "fetch" : (payload.category ?? "other")}
        />
        {browser ? "Browser" : (nouns[payload.category ?? ""] ?? "Permission")}
        {items.length > 1 ? <span>1 of {items.length}</span> : null}
      </header>
      <p className="acp-permission-prompt__question">{question}</p>
      {payload.reason ? (
        <p className="acp-permission-prompt__reason" title={payload.reason}>
          <span>Agent&apos;s reason:</span> {payload.reason}
        </p>
      ) : null}
      <Detail className="acp-permission-prompt__detail">
        {payload.summary}
      </Detail>
      {sent?.[0] === item.id && !sent[1] ? (
        <p className="acp-permission__error" role="alert">
          This decision could not be sent. Try again, or cancel the Turn.
        </p>
      ) : null}
      <p className="sr-only" role="status" aria-live="polite">
        {announcement}
      </p>
      <div className="acp-permission-prompt__actions">
        {onCancelTurn ? (
          <button
            ref={firstRef}
            type="button"
            className="acp-permission-prompt__stop"
            aria-label="Stop turn"
            onClick={onCancelTurn}
          >
            Stop
          </button>
        ) : null}
        {canDeny ? (
          <button
            ref={onCancelTurn ? undefined : firstRef}
            type="button"
            disabled={pending}
            onClick={() => resolve("deny")}
          >
            Deny <kbd>Esc</kbd>
          </button>
        ) : null}
        {primary ? (
          <span className="acp-permission-prompt__split">
            <button
              ref={primaryRef}
              type="button"
              disabled={pending}
              onClick={() => resolve(primary[0])}
            >
              {primary[1]} <kbd aria-label="Enter">⏎</kbd>
            </button>
            {choices[1] ? (
              <button
                ref={chevronRef}
                type="button"
                aria-label="More allow options"
                aria-haspopup="menu"
                aria-expanded={menuOpen}
                disabled={pending}
                onClick={() => setMenuOpen(!menuOpen)}
              >
                <ChevronDown size={14} aria-hidden="true" />
              </button>
            ) : null}
          </span>
        ) : null}
      </div>
      {menuOpen ? (
        <MenuPopover
          id={menuId}
          className="acp-permission-prompt__menu"
          ariaLabel="Allow options"
          open
          triggerRef={chevronRef}
          onDismiss={() => {
            setMenuOpen(false);
            chevronRef.current?.focus();
          }}
        >
          {choices.map(([decision, label]) => (
            <button
              key={decision}
              type="button"
              role="menuitem"
              onClick={() => resolve(decision)}
            >
              {label}
            </button>
          ))}
        </MenuPopover>
      ) : null}
    </section>
  );
}
