import { useState } from "react";
import type { AcpGoalSnapshot, GoalIdentity } from "../../domain/acpGoal";

export function GoalEditor({
  snapshot,
  disabled,
  onSave,
  onClose,
}: {
  snapshot?: AcpGoalSnapshot;
  disabled: boolean;
  onSave: (
    objective: string | undefined,
    expectedGoal?: GoalIdentity,
  ) => Promise<boolean>;
  onClose: () => void;
}) {
  const [baseline, setBaseline] = useState(snapshot);
  const [editedText, setText] = useState<string>();
  const text = editedText ?? snapshot?.objective ?? "";
  const changed =
    baseline?.objective !== snapshot?.objective ||
    baseline?.createdAt !== snapshot?.createdAt ||
    baseline?.progress?.setAt !== snapshot?.progress?.setAt;
  const textLength = [...text].length;
  const valid =
    text.trim() !== "" && textLength <= 4000 && !text.includes("\0");
  return (
    <div className="chat-goal-editor" role="group" aria-label="Edit goal">
      {changed && <p role="alert">The goal changed while you were editing</p>}
      <label>
        Goal objective
        <textarea
          autoFocus
          value={text}
          maxLength={8000}
          disabled={disabled}
          onChange={(event) =>
            setText([...event.target.value].slice(0, 4000).join(""))
          }
        />
      </label>
      <span>{textLength.toLocaleString()} / 4,000</span>
      <button
        type="button"
        disabled={disabled || !valid || changed}
        onClick={async () => {
          if (await onSave(editedText, baseline)) onClose();
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
