import { type TurnProjection } from "../../domain/acp";
import "./turn-navigator.css";
import { turnAnswerPreview, turnPrompt } from "./turnNavigatorModel";

export function TurnPreviewCard({
  id,
  turn,
  top,
}: {
  id: string;
  turn: TurnProjection;
  top: number;
}) {
  const answer = turnAnswerPreview(turn);
  return (
    <div
      id={id}
      role="tooltip"
      className="turn-navigator__preview"
      style={{ top }}
    >
      <strong>{turnPrompt(turn)}</strong>
      {answer ? <p>{answer}</p> : null}
    </div>
  );
}
