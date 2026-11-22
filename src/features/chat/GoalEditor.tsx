import { useState } from "react";
import type { AcpGoalSnapshot, GoalIdentity } from "../../domain/acpGoal";

export function GoalEditor({
  snapshot,
  canSetObjective,
  canSetBudget,
  disabled,
  onSave,
  onClose,
}: {
  snapshot?: AcpGoalSnapshot;
  canSetObjective: boolean;
  canSetBudget: boolean;
  disabled: boolean;
  onSave: (
    objective: string | undefined,
    tokenBudget: number | undefined,
    expectedGoal?: GoalIdentity,
  ) => Promise<boolean>;
  onClose: () => void;
}) {
  const [baseline, setBaseline] = useState(snapshot);
  const [editedText, setText] = useState<string>();
  const [editedBudget, setBudget] = useState<string>();
  const text = editedText ?? snapshot?.objective ?? "";
  const budget = editedBudget ?? snapshot?.tokenBudget?.toString() ?? "";
  const changed =
    baseline?.objective !== snapshot?.objective ||
    baseline?.createdAt !== snapshot?.createdAt ||
    baseline?.progress?.setAt !== snapshot?.progress?.setAt;
  const textLength = [...text].length;
  const parsedBudget =
    canSetBudget && editedBudget ? Number(editedBudget) : undefined;
  const valid =
    text.trim() !== "" &&
    textLength <= 4000 &&
    !text.includes("\0") &&
    (parsedBudget === undefined ||
      (/^\d+$/.test(budget) &&
        Number.isSafeInteger(parsedBudget) &&
        parsedBudget > 0));
  return (
    <div className="chat-goal-editor" role="group" aria-label="Edit goal">
      {changed && <p role="alert">The goal changed while you were editing</p>}
      <label>
        Goal objective
        <textarea
          autoFocus={canSetObjective}
          value={text}
          maxLength={8000}
          disabled={disabled || !canSetObjective}
          onChange={(event) =>
            setText([...event.target.value].slice(0, 4000).join(""))
          }
        />
      </label>
      <span>{textLength.toLocaleString()} / 4,000</span>
      {canSetBudget && (
        <label>
          Token budget (optional)
          <input
            aria-label="Token budget (optional)"
            autoFocus={!canSetObjective}
            inputMode="numeric"
            maxLength={17}
            value={budget}
            disabled={disabled}
            onChange={(event) => setBudget(event.target.value.slice(0, 17))}
          />
          <span>Blank keeps the budget.</span>
        </label>
      )}
      <button
        type="button"
        disabled={disabled || !valid || changed}
        onClick={async () => {
          if (
            await onSave(
              canSetObjective ? editedText : undefined,
              parsedBudget,
              baseline,
            )
          )
            onClose();
        }}
      >
        Save goal
      </button>
      {changed && (
        <button
          type="button"
          disabled={disabled}
          onClick={() => setBaseline(snapshot)}
        >
          Keep editing
        </button>
      )}
      <button type="button" disabled={disabled} onClick={onClose}>
        {changed ? "Discard" : "Cancel"}
      </button>
    </div>
  );
}
