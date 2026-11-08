import { useEffect, useId, useRef, useState } from "react";

import type {
  AcpItem,
  UserInputAnswer,
  UserInputQuestion,
  UserInputRequest,
} from "../../../domain/acp";
import { USER_INPUT_MAX_QUESTIONS } from "../../../domain/acpPermission";
import { utf8ByteLength } from "../../../domain/text";
import { acpDesktop } from "../../../services/acpDesktop";
import { approvalHold, resolveApproval } from "../useApprovalSlot";
import {
  arrivalFocusAllowed,
  logCardArrival,
  parkFocus,
  useCardKeyGuard,
} from "./approvalCard";
import { ToolKindIcon } from "./ToolItem";
import "./user-input-prompt.css";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

/** The contract admits at most this many UTF-8 bytes of answer text. */
const MAX_ANSWER_BYTES = 4096;
/** The "Other" choice beside a question's listed options. */
const OTHER = -1;

/** Free text is admitted for free-text, "other" and secret questions. */
const admitsText = (question: UserInputQuestion) =>
  !question.options || question.allowsOther || question.secret;

/** Whether a field's free text can be sent as an answer. */
type TextState = "empty" | "ok" | "tooLong";

const textState = (text: string): TextState =>
  utf8ByteLength(text) > MAX_ANSWER_BYTES
    ? "tooLong"
    : text.trim() === ""
      ? "empty"
      : "ok";

/**
 * One choice per question: a listed option's index, `OTHER` for free text,
 * or `null` while unanswered. A free-text-only question is always `OTHER`.
 */
type Choice = number | null;

function initialChoices(questions: readonly UserInputQuestion[]): Choice[] {
  return questions.map((question) => (question.options ? null : OTHER));
}

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

/**
 * A harness asking the user questions, in the composer slot. Each question
 * takes one listed option or, where the contract admits it, free text;
 * secret answers are masked and live only in the input until sent. Submit
 * stays disabled until every question is answered. Decline (or Esc) sends
 * the ordinary decline. Focus follows the approval card's arrival rules.
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
  const formId = useId();
  const sectionRef = useRef<HTMLElement>(null);
  // Held keys pressed inside the card repeat, so editing fields behave.
  const keyGuard = useCardKeyGuard({ ownRepeats: true });
  // Text stays in the (uncontrolled) inputs; React state keeps only whether
  // each one holds an answer, so a secret never enters state or the DOM text.
  const textRefs = useRef<(HTMLInputElement | null)[]>([]);
  const [choices, setChoices] = useState(() => initialChoices(questions));
  const [texts, setTexts] = useState(() =>
    questions.map((): TextState => "empty"),
  );
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
      card?.querySelector<HTMLInputElement>("input:not(:disabled)")?.focus();
    } else {
      parkFocus(card);
      setAnnounce(true);
    }
    // Focus is decided once per request; later renders never steal it.
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const pending = sent ?? approvalHold.denies.has(item.id);
  /** The option chosen for a question, if one is. */
  const optionOf = (question: UserInputQuestion, index: number) => {
    const choice = choices[index];
    return choice === null || choice === OTHER
      ? undefined
      : question.options?.[choice];
  };
  const complete = questions.every(
    (question, index) =>
      optionOf(question, index) ||
      (choices[index] === OTHER &&
        admitsText(question) &&
        texts[index] === "ok"),
  );
  /** Read at submit time: free text lives only in its input. */
  const answers = () =>
    questions.map((question, index): UserInputAnswer =>
      optionOf(question, index)
        ? {
            questionId: question.id,
            selectedOption: optionOf(question, index)!.label,
          }
        : { questionId: question.id, text: textRefs.current[index]!.value },
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
    decide(() =>
      acpDesktop.answerUserInput({
        projectId: item.projectId,
        chatId: item.chatId,
        sessionId: item.sessionId,
        turnId: item.turnId,
        runtimeEpoch: item.runtimeEpoch,
        requestId: item.payload.requestId,
        answers: answers(),
      }),
    );
  };
  const choose = (index: number, choice: Choice) =>
    setChoices((current) =>
      current.map((value, at) => (at === index ? choice : value)),
    );
  const title =
    questions.length > 1
      ? "The agent has questions"
      : "The agent has a question";
  return (
    <section
      ref={sectionRef}
      tabIndex={-1}
      className="acp-permission-prompt acp-user-input"
      role="group"
      aria-label="Agent question"
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
        Question
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
      <form
        id={formId}
        className="acp-user-input__questions"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        {questions.map((question, index) => {
          const name = `${formId}-${index}`;
          const tooLongId = `${name}-too-long`;
          const tooLong = texts[index] === "tooLong";
          const textField = admitsText(question) ? (
            <>
              <input
                ref={(input) => {
                  textRefs.current[index] = input;
                }}
                className="acp-user-input__text"
                type={question.secret ? "password" : "text"}
                aria-label={
                  question.options
                    ? `Other answer: ${question.header || question.question}`
                    : `Answer: ${question.header || question.question}`
                }
                autoComplete="off"
                spellCheck={false}
                maxLength={MAX_ANSWER_BYTES}
                disabled={pending}
                aria-invalid={tooLong || undefined}
                aria-describedby={tooLong ? tooLongId : undefined}
                // Focus alone (Tab passing through) never changes the answer;
                // a click in the field or typed text chooses "Other".
                onClick={() => {
                  if (question.options) choose(index, OTHER);
                }}
                onInput={(event) => {
                  const { value } = event.currentTarget;
                  const state = textState(value);
                  setTexts((current) =>
                    current.map((old, at) => (at === index ? state : old)),
                  );
                  if (question.options && value !== "") choose(index, OTHER);
                }}
              />
              {tooLong ? (
                <p id={tooLongId} className="acp-user-input__too-long">
                  Too long to send: answers are limited to{" "}
                  {MAX_ANSWER_BYTES.toLocaleString("en-US")} bytes.
                </p>
              ) : null}
            </>
          ) : null;
          return (
            <fieldset
              key={question.id}
              className="acp-user-input__question"
              disabled={pending}
            >
              <legend>{question.header || `Question ${index + 1}`}</legend>
              <p>{question.question}</p>
              {question.options ? (
                <div className="acp-user-input__options">
                  {question.options.map((option, at) => (
                    <label
                      key={option.label}
                      className="acp-user-input__option"
                    >
                      <input
                        type="radio"
                        name={name}
                        checked={choices[index] === at}
                        onChange={() => choose(index, at)}
                      />
                      <span>
                        <b>{option.label}</b>
                        {option.description ? (
                          <span>{option.description}</span>
                        ) : null}
                      </span>
                    </label>
                  ))}
                  {textField ? (
                    <>
                      <label className="acp-user-input__option">
                        <input
                          type="radio"
                          name={name}
                          checked={choices[index] === OTHER}
                          onChange={() => choose(index, OTHER)}
                        />
                        <span>
                          <b>Other</b>
                        </span>
                      </label>
                      {textField}
                    </>
                  ) : null}
                </div>
              ) : (
                textField
              )}
            </fieldset>
          );
        })}
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
          This answer could not be sent. Try again, or cancel the Turn.
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
            Submit <kbd aria-label="Enter">⏎</kbd>
          </button>
        </span>
      </div>
    </section>
  );
}
