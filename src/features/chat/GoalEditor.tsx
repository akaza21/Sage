import { useState } from "react";

export function GoalEditor({
  objective,
  canSetObjective,
  canSetBudget,
  tokenBudget,
  disabled,
  onSave,
  onClose,
}: {
  objective: string;
  canSetObjective: boolean;
  canSetBudget: boolean;
  tokenBudget: number | null;
  disabled: boolean;
  onSave: (objective: string, tokenBudget?: number) => Promise<boolean>;
  onClose: () => void;
}) {
  const [text, setText] = useState(objective);
  const [budget, setBudget] = useState(tokenBudget?.toString() ?? "");
  const parsedBudget =
    !canSetBudget || budget === "" ? undefined : Number(budget);
  const valid =
    text.trim().length > 0 &&
    [...text].length <= 4000 &&
    !text.includes("\0") &&
    (parsedBudget === undefined ||
      (/^\d+$/.test(budget) &&
        Number.isSafeInteger(parsedBudget) &&
        parsedBudget > 0));
  return (
    <div className="chat-goal-editor" role="group" aria-label="Edit goal">
      <label>
        Goal objective
        <textarea
          autoFocus={canSetObjective}
          aria-label="Goal objective"
          value={text}
          maxLength={8000}
          disabled={disabled || !canSetObjective}
          onChange={(event) =>
            setText([...event.target.value].slice(0, 4000).join(""))
          }
        />
      </label>
      <span>{[...text].length.toLocaleString()} / 4,000</span>
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
          <span>Leave blank to keep the current budget.</span>
        </label>
      )}
      <button
        type="button"
        disabled={disabled || !valid}
        onClick={() => {
          if (!disabled && valid)
            void onSave(text, parsedBudget).then((applied) => {
              if (applied) onClose();
            });
        }}
      >
        Save goal
      </button>
      <button type="button" disabled={disabled} onClick={onClose}>
        Cancel
      </button>
    </div>
  );
}
