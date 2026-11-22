import type { AcpGoalSnapshot } from "../../domain/acpGoal";
import "../../styles/goal-details.css";
export { GoalEditor } from "./GoalEditor";

export function GoalDetails({
  snapshot,
  canMutate,
}: {
  snapshot: AcpGoalSnapshot;
  canMutate: boolean;
}) {
  return (
    <div className="chat-goal-strip__details">
      <p>{snapshot.objective}</p>
      {!canMutate ? (
        <p>
          Native Goal actions are unavailable for this session. Reconnect or
          reload to check availability.
        </p>
      ) : null}
      {snapshot.progress?.lastReason && <p>{snapshot.progress.lastReason}</p>}
      <dl>
        <div>
          <dt>Status</dt>
          <dd>{snapshot.status}</dd>
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
