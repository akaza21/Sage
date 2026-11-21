import { useCallback, useRef, useState } from "react";
import type { AcpGoalState } from "../../domain/acpGoal";
import { canMutateGoal, goalCapabilities } from "./acp/goalMutability";
import { currentContext } from "./acp/useAcpChatActions";

export type BoundGoalDetails = ReturnType<typeof useBoundGoalDetails>;

export function useBoundGoalDetails(
  state: AcpGoalState,
  chatId: string,
  enabled: boolean,
) {
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const [ready, setReady] = useState(false);
  const onReady = useCallback(() => setReady(true), []);
  const page =
    state.kind === "goal" || state.kind === "noGoal"
      ? currentContext(chatId || undefined)?.page
      : undefined;
  const capabilities = goalCapabilities(page);
  const [editorOpen, setEditorOpen] = useState(false);
  const canMutate =
    (state.kind === "goal" && canMutateGoal(page)) ||
    (state.kind === "noGoal" && canMutateGoal(page, "set"));
  const onOpen =
    enabled && canMutate && ready
      ? () => {
          if (capabilities?.set) {
            setEditorOpen(true);
            return;
          }
          if (detailsRef.current) {
            detailsRef.current.open = true;
            detailsRef.current.querySelector("summary")?.focus();
          }
        }
      : undefined;
  return {
    detailsRef,
    onOpen,
    canMutate,
    onReady,
    capabilities,
    editorOpen,
    onEditorClose: () => setEditorOpen(false),
  };
}
