import type { WorkbenchState } from "./store";

/** Stable subscription value: typing within the same draft keeps this unchanged. */
export function unsentChatIdsJson(
  state: Pick<WorkbenchState, "chatUiById">,
): string {
  return JSON.stringify(
    Object.keys(state.chatUiById).filter((id) => {
      const input = state.chatUiById[id]!;
      return (
        input.composerText ||
        input.attachedContext.length ||
        input.queuedPrompts?.length
      );
    }),
  );
}
