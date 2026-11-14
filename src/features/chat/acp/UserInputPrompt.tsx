import { useEffect, useId, useRef, useState } from "react";

import type {
  AcpItem,
  UserInputAnswer,
  UserInputQuestion,
  UserInputRequest,
} from "../../../domain/acp";
import { USER_INPUT_MAX_QUESTIONS } from "../../../domain/acpPermission";
import { acpDesktop } from "../../../services/acpDesktop";
import { approvalHold, resolveApproval } from "../useApprovalSlot";
import {
  arrivalFocusAllowed,
  logCardArrival,
  parkFocus,
  useCardKeyGuard,
} from "./approvalCard";
import { ToolKindIcon } from "./ToolItem";
import {
  type FieldState,
  UserInputField,
  initialField,
  isAnswered,
  isSendable,
} from "./UserInputField";
import "./user-input-prompt.css";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

/** Seconds left until the request resolves itself, ticking once a second. */
function useSecondsLeft(deadline: number | null) {
  const left = () =>
    deadline === null
      ? null
      : Math.max(0, Math.ceil((deadline - Date.now()) / 1000));
  const [seconds, setSeconds] = useState(left);
  useEffect(() => {
    if (deadline === null) return;
    const timer = window.setInterval(() => {
      const next = left();
      setSeconds(next);
      if (!next) window.clearInterval(timer);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [deadline]); // eslint-disable-line react-hooks/exhaustive-deps
  return seconds;
}

function countdown(seconds: number) {
  const minutes = Math.floor(seconds / 60);
  const rest = String(seconds % 60).padStart(2, "0");
  return minutes >= 60
    ? `${Math.floor(minutes / 60)}h ${minutes % 60}m`
    : `${minutes}:${rest}`;
}

/** The wire answer for one answered question, read at submit time. */
function answerOf(
  question: UserInputQuestion,
  field: FieldState,
  text: string | undefined,
): UserInputAnswer {
  const labels = field.picked.map((at) => question.options![at]!.label);
  const typed = field.other && field.text === "ok" ? { text } : {};
  if (question.multiSelect) {
    return {
      questionId: question.id,
      ...(labels.length > 0 ? { selectedOptions: labels } : {}),
      ...typed,
    };
  }
  return labels.length > 0
    ? { questionId: question.id, selectedOption: labels[0] }
    : { questionId: question.id, ...typed };
}

/**
 * The shared card for every question a harness asks, in the composer slot:
 * questions (one or several choices, free, secret, multi-line or numeric
 * text) and links to open (sign-in). Submit stays disabled until every
 * required question is answered; a link is opened only when the user
 * presses Open. Decline (or Esc) sends the ordinary decline; stopping the
 * Turn cancels. Focus follows the approval card's arrival rules.
 */
export function UserInputPrompt({
  item,
  userInput,
  items,
  focused = true,
  canTakeFocus = () => true,
  onCancelTurn,
}: {
  item: PermissionProjectionItem;
  userInput: UserInputRequest;
  items: readonly PermissionProjectionItem[];
  focused?: boolean;
  canTakeFocus?: () => boolean;
  onCancelTurn?: () => void;
}) {
  const questions = userInput.questions.slice(0, USER_INPUT_MAX_QUESTIONS);
  const link = userInput.url;
  const formId = useId();
  const sectionRef = useRef<HTMLElement>(null);
  // Held keys pressed inside the card repeat, so editing fields behave.
  const keyGuard = useCardKeyGuard({ ownRepeats: true });
  // Text stays in the (uncontrolled) fields; React state keeps only whether
  // each one holds an answer, so a secret never enters state or the DOM text.
  const textRefs = useRef<(HTMLInputElement | HTMLTextAreaElement | null)[]>(
    [],
  );
  const [fields, setFields] = useState(() => questions.map(initialField));
  const [announce, setAnnounce] = useState(false);
  /** The decision in flight, or one that failed to send (`false`). */
  const [sent, setSent] = useState<boolean | null>(null);
  // The window runs from when the request arrived; the card is keyed per
  // request, so the deadline is fixed once.
  const [deadline] = useState(() =>
    userInput.autoResolutionMs === undefined
      ? null
      : (Number.isFinite(item.createdAt) && item.createdAt > 0
          ? item.createdAt
          : Date.now()) + userInput.autoResolutionMs,
  );
  const secondsLeft = useSecondsLeft(deadline);
  useEffect(() => {
    // A Deny pressed on the loading fallback carries into the card.
    approvalHold.denies.get(item.id)?.catch(() => setSent(false));
    const card = sectionRef.current;
    logCardArrival(item, card);
    // Each request mounts its own card (keyed), so a field of this card can
    // only hold focus from this card's own arrival (a replayed effect).
    const active = document.activeElement;
    if (active !== card && card?.contains(active)) return;
    if (arrivalFocusAllowed(item.id, card, focused, canTakeFocus)) {
      card
        ?.querySelector<HTMLElement>(
          "input:not(:disabled), textarea:not(:disabled), button[type=submit]",
        )
        ?.focus();
    } else {
      parkFocus(card);
      setAnnounce(true);
    }
    // Focus is decided once per request; later renders never steal it.
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const pending = sent ?? approvalHold.denies.has(item.id);
  const complete = questions.every(
    (question, index) =>
      isSendable(question, fields[index]!) &&
      (question.optional || isAnswered(question, fields[index]!)),
  );
  const decide = (send: () => Promise<unknown>) => {
    if (pending) return;
    approvalHold.at = Date.now();
    setSent(true);
    void send().catch(() => setSent(false));
  };
  const decline = () => decide(() => resolveApproval(item, "deny"));
  const submit = () => {
    if (!complete) return;
    const answers = questions.flatMap((question, index) =>
      isAnswered(question, fields[index]!)
        ? [answerOf(question, fields[index]!, textRefs.current[index]?.value)]
        : [],
    );
    decide(() =>
      acpDesktop.answerUserInput({
        projectId: item.projectId,
        chatId: item.chatId,
        sessionId: item.sessionId,
        turnId: item.turnId,
        runtimeEpoch: item.runtimeEpoch,
        requestId: item.payload.requestId,
        answers,
      }),
    );
  };
  const title = link
    ? "The agent asks you to open a link"
    : questions.length > 1
      ? "The agent has questions"
      : "The agent has a question";
  return (
    <section
      ref={sectionRef}
      tabIndex={-1}
      className="acp-permission-prompt acp-user-input"
      role="group"
      aria-label={link ? "Agent link" : "Agent question"}
      data-acp-item-id={item.id}
      {...keyGuard}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        decline();
      }}
    >
      <header className="acp-permission-prompt__kind">
        <ToolKindIcon kind="other" />
        {link ? "Sign-in" : "Question"}
        {items.length > 1 ? <span>1 of {items.length}</span> : null}
        {onCancelTurn ? (
          <button
            type="button"
            className="acp-permission-prompt__stop"
            aria-label="Stop turn"
            onClick={onCancelTurn}
          >
            Stop
          </button>
        ) : null}
      </header>
      <p className="acp-permission-prompt__question">{title}</p>
      {item.payload.reason ? (
        <p className="acp-user-input__message">{item.payload.reason}</p>
      ) : null}
      {link ? (
        <p className="acp-user-input__link">
          Opens <b>{link.host}</b> in your browser:
          <code>{link.url}</code>
        </p>
      ) : null}
      <form
        id={formId}
        className="acp-user-input__questions"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        {questions.map((question, index) => (
          <UserInputField
            key={question.id}
            question={question}
            index={index}
            name={`${formId}-${index}`}
            field={fields[index]!}
            pending={pending}
            textRef={(input) => {
              textRefs.current[index] = input;
            }}
            onChange={(next) =>
              setFields((current) =>
                current.map((old, at) => (at === index ? next : old)),
              )
            }
          />
        ))}
      </form>
      {secondsLeft !== null ? (
        <p className="acp-user-input__timer">
          {secondsLeft > 0
            ? `Closes unanswered in ${countdown(secondsLeft)}`
            : "Closing unanswered…"}
        </p>
      ) : null}
      {sent === false ? (
        <p className="acp-permission__error" role="alert">
          This answer could not be sent. Try again, or cancel the turn.
        </p>
      ) : null}
      <p className="sr-only" role="status">
        {announce ? `Question from the agent: ${title}` : null}
      </p>
      <div className="acp-permission-prompt__actions">
        <button
          className="button--compact"
          type="button"
          disabled={pending}
          onClick={decline}
        >
          Decline <kbd>Esc</kbd>
        </button>
        <span className="acp-permission-prompt__split">
          <button
            className="button--compact"
            type="submit"
            form={formId}
            disabled={pending || !complete}
          >
            {link ? "Open link" : "Submit"} <kbd aria-label="Enter">⏎</kbd>
          </button>
        </span>
      </div>
    </section>
  );
}
