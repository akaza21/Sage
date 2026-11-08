import type { UserInputQuestion } from "../../../domain/acp";
import { USER_INPUT_TEXT_BYTES } from "../../../domain/acpPermission";
import { utf8ByteLength } from "../../../domain/text";

/** Whether a field's free text can be sent as an answer. */
export type TextState = "empty" | "ok" | "tooLong" | "notNumber";

/** One question's answer: chosen option indexes and whether "Other" is on. */
export interface FieldState {
  picked: readonly number[];
  other: boolean;
  text: TextState;
}

/** Free text is admitted for free-text, "other" and secret questions. */
export const admitsText = (question: UserInputQuestion) =>
  !question.options || question.allowsOther || question.secret;

export function textState(
  question: UserInputQuestion,
  text: string,
): TextState {
  if (utf8ByteLength(text) > USER_INPUT_TEXT_BYTES) return "tooLong";
  const trimmed = text.trim();
  if (trimmed === "") return "empty";
  const numeric = question.kind === "number" || question.kind === "integer";
  const value = Number(trimmed);
  if (
    numeric &&
    !question.allowsOther &&
    (!Number.isFinite(value) ||
      (question.kind === "integer" && !Number.isSafeInteger(value)))
  ) {
    return "notNumber";
  }
  return "ok";
}

export function initialField(question: UserInputQuestion): FieldState {
  return {
    picked: [],
    other: !question.options,
    text: textState(question, question.initialText ?? ""),
  };
}

export const isAnswered = (question: UserInputQuestion, field: FieldState) =>
  field.picked.length > 0 ||
  (field.other && admitsText(question) && field.text === "ok");

/** Nothing typed into the field blocks sending. */
export const isSendable = (question: UserInputQuestion, field: FieldState) =>
  !field.other ||
  field.text === "ok" ||
  field.text === "empty" ||
  !admitsText(question);

const PROBLEM: Partial<Record<TextState, string>> = {
  tooLong: `Too long to send: answers are limited to ${USER_INPUT_TEXT_BYTES.toLocaleString("en-US")} bytes.`,
  notNumber: "Enter a number.",
};

/**
 * One question of the shared question card: radio buttons (one choice),
 * checkboxes (several), and a text field where free text is admitted —
 * masked for secrets, multi-line for editors, numeric for numbers. Text
 * stays in the uncontrolled field; only its state is lifted.
 */
export function UserInputField({
  question,
  index,
  name,
  field,
  pending,
  textRef,
  onChange,
}: {
  question: UserInputQuestion;
  index: number;
  name: string;
  field: FieldState;
  pending: boolean;
  textRef: (input: HTMLInputElement | HTMLTextAreaElement | null) => void;
  onChange: (next: FieldState) => void;
}) {
  const problemId = `${name}-problem`;
  const problem = field.other ? PROBLEM[field.text] : undefined;
  const label = question.header || question.question || `Question ${index + 1}`;
  const choose = (at: number) =>
    onChange(
      question.multiSelect
        ? {
            ...field,
            picked: field.picked.includes(at)
              ? field.picked.filter((value) => value !== at)
              : [...field.picked, at],
          }
        : { ...field, picked: [at], other: false },
    );
  const chooseOther = (on: boolean) =>
    onChange({
      ...field,
      other: on,
      picked: question.multiSelect || !on ? field.picked : [],
    });
  const textProps = {
    ref: textRef,
    className: "acp-user-input__text",
    "aria-label": question.options
      ? `Other answer: ${label}`
      : `Answer: ${label}`,
    autoComplete: "off",
    spellCheck: false,
    defaultValue: question.initialText,
    disabled: pending,
    "aria-invalid": problem ? true : undefined,
    "aria-describedby": problem ? problemId : undefined,
    // Focus alone (Tab passing through) never changes the answer; a click
    // in the field or typed text chooses "Other".
    onClick: () => {
      if (question.options && !field.other) chooseOther(true);
    },
    onInput: (event: { currentTarget: { value: string } }) => {
      const { value } = event.currentTarget;
      onChange({
        ...field,
        text: textState(question, value),
        other: field.other || (!!question.options && value !== ""),
        picked:
          question.options && value !== "" && !question.multiSelect
            ? []
            : field.picked,
      });
    },
  };
  const textField = admitsText(question) ? (
    <>
      {question.kind === "multiline" ? (
        <textarea {...textProps} rows={6} />
      ) : (
        <input
          {...textProps}
          type={question.secret ? "password" : "text"}
          inputMode={
            question.kind === "number"
              ? "decimal"
              : question.kind === "integer"
                ? "numeric"
                : undefined
          }
        />
      )}
      {problem ? (
        <p id={problemId} className="acp-user-input__too-long">
          {problem}
        </p>
      ) : null}
    </>
  ) : null;
  const choiceType = question.multiSelect ? "checkbox" : "radio";
  return (
    <fieldset className="acp-user-input__question" disabled={pending}>
      <legend>
        {question.header || `Question ${index + 1}`}
        {question.optional ? <span> (optional)</span> : null}
      </legend>
      {question.question ? <p>{question.question}</p> : null}
      {question.options ? (
        <div className="acp-user-input__options">
          {question.options.map((option, at) => (
            <label key={option.label} className="acp-user-input__option">
              <input
                type={choiceType}
                name={name}
                checked={field.picked.includes(at)}
                onChange={() => choose(at)}
              />
              <span>
                <b>{option.label}</b>
                {option.description ? <span>{option.description}</span> : null}
              </span>
            </label>
          ))}
          {textField ? (
            <>
              <label className="acp-user-input__option">
                <input
                  type={choiceType}
                  name={name}
                  checked={field.other}
                  onChange={() =>
                    chooseOther(question.multiSelect ? !field.other : true)
                  }
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
}
