import type { AcpGoalSnapshot } from "../../domain/acpGoal";
import "../../styles/goal-details.css";
export { GoalEditor } from "./GoalEditor";

export function GoalDetails({
  snapshot,
  statusLabel,
  canMutate,
}: {
  snapshot: AcpGoalSnapshot;
  statusLabel: string;
  canMutate: boolean;
}) {
  return (
    <div className="chat-goal-strip__details">
      <p>{snapshot.objective}</p>
      {!canMutate ? (
        <p>Native Goal actions are unavailable. Reload to check.</p>
      ) : null}
      {snapshot.progress?.lastReason && <p>{snapshot.progress.lastReason}</p>}
      <dl>
        <div>
          <dt>Status</dt>
          <dd>{statusLabel}</dd>
        </div>
        {!snapshot.progress && (
          <div>
            <dt>Usage</dt>
            <dd>
              {snapshot.tokensUsed.toLocaleString()} /{" "}
              {snapshot.tokenBudget?.toLocaleString() ?? "unlimited"} tokens
            </dd>
          </div>
        )}
      </dl>
    </div>
  );
}
