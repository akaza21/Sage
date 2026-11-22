import { Maximize2, Trash2 } from "../../ui/leanIcons";
import { Pause, Target } from "../../ui/leanIconNodesLazy";
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AcpGoalCapabilities,
  AcpGoalState,
  GoalMutationAction,
} from "../../domain/acpGoal";
import { formatElapsedSeconds } from "../../domain/duration";
import "../../styles/chat-goal.css";
import type * as GoalContentChunk from "./GoalDetails";
import { lazyParts } from "./lazyParts";
const GoalContent = lazyParts<typeof GoalContentChunk>(
  () => import("./GoalDetails"),
);

import type { GoalControls } from "./useBoundGoalDetails";

export type GoalMutationOutcome =
  { kind: "applied" } | { kind: "refused" | "outcomeUnknown"; message: string };

function availability(state: Exclude<AcpGoalState, { kind: "goal" }>): string {
  switch (state.kind) {
    case "unsupported":
      return "Goals are not available for this binding.";
    case "offline":
      return "Chat offline. Reconnect to check Goal.";
    case "loading":
      return "Checking native Goal…";
    case "unavailable":
      return "Native Goal unavailable.";
    case "noGoal":
      return "No native Goal is set.";
  }
}

export function ChatGoalControl({
  state,
  capabilities,
  editorOpen = false,
  onEditorClose,
  onReady,
  canMutate = false,
  onMutation,
}: {
  state: AcpGoalState;
  capabilities?: AcpGoalCapabilities;
  editorOpen?: boolean;
  onEditorClose?: () => void;
  onReady?: (controls: GoalControls) => () => void;
  canMutate?: boolean;
  onMutation?: (
    action: GoalMutationAction,
    objective?: string,
  ) => Promise<GoalMutationOutcome>;
}) {
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const [mutation, setMutation] = useState<
    | { kind: "idle" | "pending" }
    | { kind: "refused" | "outcomeUnknown"; message: string }
  >({ kind: "idle" });
  const [editing, setEditing] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const controls = canMutate && onMutation;
  const locked =
    mutation.kind === "pending" || mutation.kind === "outcomeUnknown";
  const run = useCallback(
    async (
      action: GoalMutationAction,
      objective?: string,
    ): Promise<boolean> => {
      if (!controls || locked || !capabilities?.[action]) return false;
      setMutation({ kind: "pending" });
      try {
        const outcome = await controls(
          action,
          ...(action === "set" ? [objective] : []),
        );
        setMutation(outcome.kind === "applied" ? { kind: "idle" } : outcome);
        return outcome.kind === "applied";
      } catch {
        setMutation({
          kind: "outcomeUnknown",
          message: "Goal outcome unknown. Reload this chat to verify.",
        });
        return false;
      }
    },
    [controls, locked, capabilities],
  );
  useEffect(
    () =>
      onReady?.({
        goalUnobserved: capabilities?.observes === false,
        onOpenGoal:
          capabilities?.set || state.kind === "goal"
            ? () => {
                if (capabilities?.set) setEditing(true);
                else if (detailsRef.current) {
                  detailsRef.current.open = true;
                  detailsRef.current.querySelector("summary")?.focus();
                }
              }
            : undefined,
        onClearGoal:
          !capabilities?.observes && capabilities?.clear
            ? () => {
                void run("clear");
              }
            : undefined,
      }),
    [onReady, capabilities, state.kind, run],
  );
  const closeEditor = () => {
    setEditing(false);
    onEditorClose?.();
  };
  const editor =
    controls && capabilities?.set && (editing || editorOpen) ? (
      <GoalContent
        part="GoalEditor"
        objective={state.kind === "goal" ? state.snapshot.objective : ""}
        disabled={locked}
        onSave={(objective) => run("set", objective)}
        onClose={closeEditor}
      />
    ) : null;
  const notice =
    mutation.kind !== "idle" ? (
      <p
        className="chat-goal-strip__notice"
        role={mutation.kind === "pending" ? "status" : "alert"}
      >
        {"message" in mutation ? mutation.message : "Updating native Goal..."}
      </p>
    ) : null;
  if (state.kind !== "goal") {
    if (
      capabilities &&
      (!capabilities.observes || (state.kind === "noGoal" && capabilities.set))
    )
      return (
        <>
          {editor}
          {notice}
        </>
      );
    return (
      <details className="chat-goal-entry">
        <summary>
          <Target size={13} aria-hidden="true" /> Goal
        </summary>
        <p>{availability(state)}</p>
      </details>
    );
  }
  const { snapshot } = state;
  const actionButton = (
    action: "pause" | "clear",
    label: string,
    Icon: typeof Pause,
  ) => (
    <button
      type="button"
      className="chat-goal-strip__icon"
      aria-label={label}
      title={label}
      disabled={locked}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        void run(action);
      }}
    >
      <Icon size={13} aria-hidden="true" />
    </button>
  );
  return (
    <div className="chat-goal-container">
      <details
        className="chat-goal-strip"
        ref={detailsRef}
        onToggle={(event) => setDetailsOpen(event.currentTarget.open)}
      >
        <summary className="chat-goal-strip__row" aria-label="Goal details">
          <Target size={13} aria-hidden="true" />
          <strong>
            {snapshot.status === "active" ? "Pursuing goal" : "Goal"}
          </strong>
          <span
            className="chat-goal-strip__objective"
            title={snapshot.objective}
          >
            {snapshot.objective}
          </span>
          {snapshot.progress ? (
            <span>
              {snapshot.progress.iterations.toLocaleString()} iterations
            </span>
          ) : (
            <time>{formatElapsedSeconds(snapshot.timeUsedSeconds)}</time>
          )}
          <span className="chat-goal-strip__icons">
            {controls && capabilities?.set && (
              <button
                type="button"
                className="chat-goal-strip__icon"
                aria-label="Edit goal"
                disabled={locked}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  setEditing(true);
                }}
              >
                Edit
              </button>
            )}
            {controls &&
              capabilities?.pause &&
              snapshot.status === "active" &&
              actionButton("pause", "Pause goal", Pause)}
            {controls &&
              capabilities?.clear &&
              actionButton("clear", "Delete goal", Trash2)}
            <button
              type="button"
              className="chat-goal-strip__icon"
              aria-label="Toggle goal details"
              title="Expand goal details"
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                if (detailsRef.current)
                  detailsRef.current.open = !detailsRef.current.open;
              }}
            >
              <Maximize2 size={13} aria-hidden="true" />
            </button>
          </span>
        </summary>
        {detailsOpen && (
          <GoalContent
            part="GoalDetails"
            snapshot={snapshot}
            canMutate={canMutate}
          />
        )}
      </details>
      {editor}
      {notice}
    </div>
  );
}
