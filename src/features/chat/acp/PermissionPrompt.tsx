import { useEffect, useId, useRef, useState } from "react";
import { ChevronDown } from "../../../ui/leanIcons";

import type { AcpItem, PermissionDecision } from "../../../domain/acp";
import { MenuPopover } from "../../../ui/MenuPopover";
import { approvalHold, resolveApproval } from "../useApprovalSlot";
import {
  arrivalFocusAllowed,
  logCardArrival,
  parkFocus,
  useCardKeyGuard,
} from "./approvalCard";
import { ToolKindIcon } from "./ToolItem";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

const nouns: Record<string, string> = {
  execute: "Terminal",
  edit: "Edit",
  mcp: "MCP",
};

export function PermissionPrompt({
  items,
  focused = true,
  canTakeFocus = () => true,
  onCancelTurn,
}: {
  items: readonly PermissionProjectionItem[];
  focused?: boolean;
  canTakeFocus?: () => boolean;
  onCancelTurn?: () => void;
}) {
  const item = items[0];
  const primaryRef = useRef<HTMLButtonElement>(null);
  const firstRef = useRef<HTMLButtonElement>(null);
  const chevronRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const keyGuard = useCardKeyGuard();
  const sectionRef = useRef<HTMLElement>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [focusState, setFocusState] = useState<[string, boolean] | null>(null);
  /** The request in flight, or the one whose decision failed (negated). */
  const [sent, setSent] = useState<[string, boolean] | null>(null);
  const execute = item?.payload.category === "execute";
  /** A developer-mode card (ADR-0009b): CDP for this tab and site. */
  const developer = item?.payload.source === "browserDeveloper";
  const question = developer
    ? "Allow developer mode on this site?"
    : execute
      ? "Allow this command?"
      : "Allow this action?";
  useEffect(() => {
    if (!item) return;
    // A Deny pressed on the loading fallback carries into the card.
    const denied = approvalHold.denies.get(item.id);
    denied?.catch(() => setSent([item.id, false]));
    const card = sectionRef.current;
    logCardArrival(item, card);
    if (arrivalFocusAllowed(item.id, card, focused, canTakeFocus)) {
      (primaryRef.current ?? firstRef.current)?.focus();
      setFocusState([item.id, document.activeElement === primaryRef.current]);
    } else {
      parkFocus(card);
      setFocusState([item.id, false]);
    }
    // Focus is decided once per request; later renders never steal it.
  }, [item?.id]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!item) return null;
  const initialFocus = focusState?.[0] === item.id ? focusState[1] : null;
  const { payload } = item;
  const browser = payload.source === "browser" || developer;
  const choices: [PermissionDecision, string][] = [];
  for (const kind of developer
    ? ["developer"]
    : browser
      ? ["allow_once", "origin"]
      : new Set(payload.offeredOptionKinds))
    if (kind === "developer")
      choices.push(["allowDeveloperMode", "Allow developer mode"]);
    else if (kind === "allow_once") choices.push(["allowOnce", "Allow once"]);
    else if (kind === "origin")
      choices.push(["allowForOriginSession", "Allow for this tab and origin"]);
    else if (kind === "allow_always")
      choices.push([
        "allowAlways",
        execute ? "Allow similar commands" : "Always allow",
      ]);
  const primary =
    choices.find(([decision]) => decision === "allowOnce") ?? choices[0];
  const offered = payload.offeredOptionKinds;
  const canDeny =
    browser ||
    !offered ||
    offered.includes("reject_once") ||
    offered.includes("reject_always");
  const pending =
    sent?.[0] === item.id ? sent[1] : approvalHold.denies.has(item.id);
  const resolve = (decision: PermissionDecision) => {
    setMenuOpen(false);
    if (pending) return;
    approvalHold.at = Date.now();
    setSent([item.id, true]);
    void resolveApproval(item, decision).catch(() => setSent([item.id, false]));
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
      {...keyGuard}
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
        {developer ? (
          <b className="acp-permission-prompt__elevated">Elevated</b>
        ) : null}
        {items.length > 1 ? <span>1 of {items.length}</span> : null}
        {onCancelTurn ? (
          // Always offered so the pane is never stranded, although the
          // reference card has no Stop.
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
      {developer ? (
        <p className="acp-permission-prompt__risk">
          The agent gets allowlisted Chrome DevTools Protocol access to this
          tab. Scripts run as the signed-in user and can read data available to
          the page until you stop it, take over or leave the site. Only allow
          this for sites you trust.
        </p>
      ) : null}
      {sent?.[0] === item.id && !sent[1] ? (
        <p className="acp-permission__error" role="alert">
          This decision could not be sent. Try again, or cancel the turn.
        </p>
      ) : null}
      <p className="sr-only" role="status">
        {initialFocus === false ? `Approval needed: ${question}` : null}
      </p>
      <div className="acp-permission-prompt__actions">
        {canDeny ? (
          <button
            ref={onCancelTurn ? undefined : firstRef}
            className="button--compact"
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
              ref={developer ? undefined : primaryRef}
              className="button--compact"
              type="button"
              data-initial-focus={initialFocus || undefined}
              onBlur={() => setFocusState(null)}
              disabled={pending}
              onClick={() => resolve(primary[0])}
            >
              {primary[1]}
              {developer ? null : (
                <>
                  {" "}
                  <kbd aria-label="Enter">⏎</kbd>
                </>
              )}
            </button>
            {choices[1] ? (
              <button
                ref={chevronRef}
                className="button--compact"
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
              className="button button--compact"
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
