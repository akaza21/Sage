import { useEffect, useRef, useState } from "react";

import type { AcpItem, PermissionDecision } from "../../../domain/acp";
import { approvalHold, resolveApproval } from "../useApprovalSlot";
import {
  arrivalFocusAllowed,
  logCardArrival,
  parkFocus,
  useCardKeyGuard,
} from "./approvalCard";
import { ToolKindIcon } from "./ToolKindIcon";
import { leavePlanAccess } from "./leavePlanAccess";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

interface Choice {
  decision: PermissionDecision;
  label: string;
  scope?: string;
}

/** Options 1 to 4 answer to their digit keys. */
const DIGIT_KEYS = 4;

/**
 * The approval card above the composer: the request, then the harness's own
 * decisions as numbered options (the first preselected), Skip and Submit.
 * A reason with a refusal and "always allow" scopes appear only where the
 * request reports them.
 */
export function PermissionPrompt({
  items,
  focused = true,
  canTakeFocus = () => true,
}: {
  items: readonly PermissionProjectionItem[];
  focused?: boolean;
  canTakeFocus?: () => boolean;
  onCancelTurn?: () => void;
}) {
  const item = items[0];
  const optionsRef = useRef<HTMLDivElement>(null);
  const reasonRef = useRef<HTMLTextAreaElement>(null);
  // Held keys repeat only into the refusal's write-in, for editing.
  const keyGuard = useCardKeyGuard({
    ownRepeats: !!item?.payload.acceptsDenyReason,
  });
  const sectionRef = useRef<HTMLElement>(null);
  const [picked, setPicked] = useState(0);
  const [focusState, setFocusState] = useState<[string, boolean] | null>(null);
  /** The request in flight, or the one whose decision failed (negated). */
  const [sent, setSent] = useState<[string, boolean] | null>(null);
  const execute = item?.payload.category === "execute";
  /** A developer-mode card (ADR-0009b): CDP for this tab and site. */
  const developer = item?.payload.source === "browserDeveloper";
  /** A mode switch asks its own question; the text after it (a plan) is
   * what the person approves. */
  const switchMode = item?.payload.category === "switchMode";
  const [asked, ...rest] = switchMode
    ? (item?.payload.summary ?? "").split("\n\n")
    : [];
  const plan = rest.length > 0;
  const question = developer
    ? "Allow developer mode on this site?"
    : execute
      ? "Allow this command?"
      : plan
        ? asked
        : "Allow this action?";
  const detail = plan ? rest.join("\n\n") : item?.payload.summary;
  const option = (index: number) =>
    optionsRef.current?.querySelectorAll<HTMLButtonElement>("[role=radio]")[
      index
    ];
  useEffect(() => {
    if (!item) return;
    // A Deny pressed on the loading fallback carries into the card.
    const denied = approvalHold.denies.get(item.id);
    denied?.catch(() => setSent([item.id, false]));
    const card = sectionRef.current;
    logCardArrival(item, card);
    if (arrivalFocusAllowed(item.id, card, focused, canTakeFocus)) {
      option(0)?.focus();
      setFocusState([item.id, document.activeElement === option(0)]);
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
  const choices: Choice[] = [];
  for (const kind of developer
    ? ["developer"]
    : browser
      ? ["allow_once", "origin"]
      : new Set(payload.offeredOptionKinds))
    if (kind === "developer")
      choices.push({
        decision: "allowDeveloperMode",
        label: "Allow developer mode",
      });
    else if (kind === "allow_once")
      choices.push({ decision: "allowOnce", label: "Allow once" });
    else if (kind === "origin")
      choices.push({
        decision: "allowForOriginSession",
        label: "Allow for this tab and origin",
      });
    else if (kind === "allow_always" && !payload.alwaysAllowScopes?.length)
      choices.push({
        decision: "allowAlways",
        label: execute ? "Allow similar commands" : "Always allow",
      });
  for (const scope of browser ? [] : (payload.alwaysAllowScopes ?? []))
    choices.push({
      decision: "allowAlways",
      label: scope.label,
      scope: scope.id,
    });
  const offered = payload.offeredOptionKinds;
  const canDeny =
    browser ||
    !offered ||
    offered.includes("reject_once") ||
    offered.includes("reject_always");
  const writeIn = canDeny && !browser && !!payload.acceptsDenyReason;
  if (canDeny)
    choices.push({
      decision: "deny",
      label: writeIn ? "No" : "Deny",
    });
  const current = Math.min(picked, choices.length - 1);
  const pending =
    sent?.[0] === item.id ? sent[1] : approvalHold.denies.has(item.id);
  const resolve = (choice: Choice | undefined) => {
    if (pending || !choice) return;
    approvalHold.at = Date.now();
    setSent([item.id, true]);
    const reason =
      choice.decision === "deny" && writeIn
        ? reasonRef.current?.value.trim()
        : undefined;
    void resolveApproval(item, choice.decision, {
      ...(reason ? { reason } : {}),
      ...(choice.scope ? { scope: choice.scope } : {}),
    }).then(
      () => {
        if (switchMode) void leavePlanAccess(item.sessionId, choice.decision);
      },
      () => setSent([item.id, false]),
    );
  };
  const pick = (index: number) => {
    setPicked(index);
    option(index)?.focus();
  };
  const Detail = execute || item.payload.category === "extension" ? "pre" : "p";
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
        const typing = event.target instanceof HTMLTextAreaElement;
        const digit = Number(event.key);
        if (event.key === "Escape" && canDeny) {
          event.preventDefault();
          resolve({ decision: "deny", label: "Deny" });
        } else if (
          event.key === "Enter" &&
          !event.shiftKey &&
          // Skip and Submit answer Enter as their own click.
          !(
            event.target instanceof HTMLButtonElement &&
            event.target.getAttribute("role") !== "radio"
          )
        ) {
          event.preventDefault();
          resolve(choices[current]);
        } else if (
          !typing &&
          Number.isInteger(digit) &&
          digit >= 1 &&
          digit <= Math.min(DIGIT_KEYS, choices.length)
        ) {
          event.preventDefault();
          pick(digit - 1);
        } else if (
          !typing &&
          (event.key === "ArrowDown" || event.key === "ArrowUp")
        ) {
          event.preventDefault();
          const step = event.key === "ArrowDown" ? 1 : choices.length - 1;
          pick((current + step) % choices.length);
        }
      }}
    >
      <header className="acp-permission-prompt__kind">
        <ToolKindIcon
          kind={browser ? "fetch" : (payload.category ?? "other")}
        />
        <p className="acp-permission-prompt__question">{question}</p>
        {developer ? (
          <b className="acp-permission-prompt__elevated">Elevated</b>
        ) : null}
        {items.length > 1 ? <span>1 of {items.length}</span> : null}
      </header>
      {payload.reason ? (
        <p className="acp-permission-prompt__reason" title={payload.reason}>
          <span>Agent&apos;s reason:</span> {payload.reason}
        </p>
      ) : null}
      {detail ? (
        <Detail
          className={`acp-permission-prompt__detail${plan ? " acp-permission-prompt__detail--plan" : ""}`}
        >
          {detail}
        </Detail>
      ) : null}
      {developer ? (
        <p className="acp-permission-prompt__risk">
          The agent gets allowlisted Chrome DevTools Protocol access to this
          tab. Scripts run as the signed-in user and can read data available to
          the page until you stop it, take over or leave the site. Only allow
          this for sites you trust.
        </p>
      ) : null}
      <div
        ref={optionsRef}
        className="acp-permission-prompt__options"
        role="radiogroup"
        aria-label="Decision"
      >
        {choices.map((choice, index) => (
          <button
            key={`${choice.decision}:${choice.scope ?? ""}`}
            type="button"
            role="radio"
            className="acp-permission-prompt__option"
            aria-checked={index === current}
            tabIndex={index === current ? 0 : -1}
            data-initial-focus={
              (index === 0 && initialFocus && index === current) || undefined
            }
            disabled={pending}
            onBlur={() => index === 0 && setFocusState(null)}
            onClick={() => setPicked(index)}
          >
            <kbd aria-hidden="true">{index + 1}</kbd>
            {choice.label}
          </button>
        ))}
      </div>
      {writeIn ? (
        // The refusal's reason: typing in it picks No.
        <textarea
          ref={reasonRef}
          className="acp-permission-prompt__write-in"
          aria-label="What the agent should do instead"
          placeholder="(tell the agent what to do instead)"
          rows={1}
          maxLength={2000}
          disabled={pending}
          onFocus={() => setPicked(choices.length - 1)}
        />
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
            className="button--compact"
            type="button"
            disabled={pending}
            onClick={() => resolve({ decision: "deny", label: "Deny" })}
          >
            Skip
          </button>
        ) : null}
        <button
          className="button--compact acp-permission-prompt__submit"
          type="button"
          disabled={pending || choices.length === 0}
          onClick={() => resolve(choices[current])}
        >
          Submit <span aria-hidden="true">↵</span>
        </button>
      </div>
    </section>
  );
}
