import type { UserInputQuestion } from "../../../domain/acp";
import { USER_INPUT_TEXT_BYTES } from "../../../domain/acpPermission";
import { utf8ByteLength } from "../../../domain/text";

export type TextState =
  "empty" | "ok" | "tooLong" | "notNumber" | "badLength" | "outOfRange";

export interface FieldState {
  picked: readonly number[];
  other: boolean;
  text: TextState;
}

export const admitsText = (question: UserInputQuestion) =>
  !question.options || question.allowsOther || question.secret;

const INTEGER = /^[+-]?\d+$/u;
const NUMBER = /^[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?$/u;
const I64 = 2n ** 63n;

// Must match the native number grammar (`limits.rs`).
export function parseAnswerNumber(
  kind: "number" | "integer",
  text: string,
): bigint | number | null {
  if (kind === "integer") {
    if (!INTEGER.test(text)) return null;
    const value = BigInt(text);
    return value >= -I64 && value < I64 ? value : null;
  }
  const value = NUMBER.test(text) ? Number(text) : NaN;
  return Number.isFinite(value) ? value : null;
}

const compare = (value: bigint | number, bound: number) =>
  typeof value === "bigint" && Number.isInteger(bound)
    ? Math.sign(Number(value - BigInt(bound)))
    : Math.sign(Number(value) - bound);

const within = (count: number, min?: number, max?: number) =>
  (min === undefined || count >= min) && (max === undefined || count <= max);

export function textState(
  question: UserInputQuestion,
  text: string,
): TextState {
  if (utf8ByteLength(text) > USER_INPUT_TEXT_BYTES) return "tooLong";
  const trimmed = text.trim();
  if (trimmed === "") return "empty";
  if (question.allowsOther) return "ok";
  const limits = question.limits ?? {};
  if (!within([...text].length, limits.minLength, limits.maxLength)) {
    return "badLength";
  }
  if (question.kind !== "number" && question.kind !== "integer") return "ok";
  const value = parseAnswerNumber(question.kind, trimmed);
  if (value === null) return "notNumber";
  const below =
    limits.minimum !== undefined && compare(value, limits.minimum) < 0;
  const above =
    limits.maximum !== undefined && compare(value, limits.maximum) > 0;
  return below || above ? "outOfRange" : "ok";
}

export const itemsOk = (question: UserInputQuestion, field: FieldState) =>
  !question.multiSelect ||
  within(
    field.picked.length,
    question.limits?.minItems,
    question.limits?.maxItems,
  );

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

export const isSendable = (question: UserInputQuestion, field: FieldState) =>
  (!field.other ||
    field.text === "ok" ||
    field.text === "empty" ||
    !admitsText(question)) &&
  (!isAnswered(question, field) || itemsOk(question, field));

function span(min?: number, max?: number) {
  if (min !== undefined && max !== undefined) {
    return min === max ? `exactly ${min}` : `from ${min} to ${max}`;
  }
  return min !== undefined ? `at least ${min}` : `at most ${max}`;
}

function textProblem(question: UserInputQuestion, state: TextState) {
  const limits = question.limits ?? {};
  switch (state) {
    case "tooLong":
      return `Too long to send: answers are limited to ${USER_INPUT_TEXT_BYTES.toLocaleString("en-US")} bytes.`;
    case "notNumber":
      return "Enter a number.";
    case "badLength":
      return `Enter ${span(limits.minLength, limits.maxLength)} characters.`;
    case "outOfRange":
      return `Enter a number ${span(limits.minimum, limits.maximum)}.`;
    default:
      return undefined;
  }
}

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
  const limits = question.limits ?? {};
  const textIssue =
    field.other && admitsText(question)
      ? textProblem(question, field.text)
      : undefined;
  const problem =
    textIssue ??
    (isAnswered(question, field) && !itemsOk(question, field)
      ? `Choose ${span(limits.minItems, limits.maxItems)} options.`
      : undefined);
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
    "aria-invalid": textIssue ? true : undefined,
    "aria-describedby": textIssue ? problemId : undefined,
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
      {problem ? (
        <p id={problemId} className="acp-user-input__too-long">
          {problem}
        </p>
      ) : null}
    </fieldset>
  );
}
