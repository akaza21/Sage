import type { WorkbenchState } from "./store";

/** Stable subscription value: typing within the same draft keeps this unchanged. */
export function unsentChatIdsJson(
  state: Pick<WorkbenchState, "chatUiById">,
): string {
  const ids: string[] = [];
  for (const [id, input] of Object.entries(state.chatUiById)) {
    if (
      input.composerText ||
      input.attachedContext.length ||
      input.queuedPrompts?.length
    ) {
      ids.push(id);
    }
  }
  return JSON.stringify(ids);
}
