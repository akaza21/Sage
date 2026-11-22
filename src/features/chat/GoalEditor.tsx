import { useState } from "react";

export function GoalEditor({
  objective,
  disabled,
  onSave,
  onClose,
}: {
  objective: string;
  disabled: boolean;
  onSave: (objective: string) => Promise<boolean>;
  onClose: () => void;
}) {
  const [text, setText] = useState(objective);
  const valid =
    text.trim().length > 0 && [...text].length <= 4000 && !text.includes("\0");
  return (
    <div className="chat-goal-editor" role="group" aria-label="Edit goal">
      <label>
        Goal objective
        <textarea
          autoFocus
          aria-label="Goal objective"
          value={text}
          maxLength={8000}
          disabled={disabled}
          onChange={(event) =>
            setText([...event.target.value].slice(0, 4000).join(""))
          }
        />
      </label>
      <span>{[...text].length.toLocaleString()} / 4,000</span>
      <button
        type="button"
        disabled={disabled || !valid}
        onClick={() => {
          if (!disabled && valid)
            void onSave(text).then((applied) => {
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
