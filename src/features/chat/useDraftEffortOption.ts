import { useEffect } from "react";
import {
  draftChatEffort,
  useChatEffortDrafts,
} from "../../app/chatEffortDrafts";
import type {
  AcpConfigOption,
  AcpConfigValue,
} from "../../domain/acpSessionConfig";
import type { ModelOption } from "../../domain/workbench";
import { capitalizeLabel } from "../../domain/text";

/** The composer's own id for a thinking level held before a session. */
const DRAFT_EFFORT_ID = "sage-draft-effort";
/** The level that leaves the choice to the harness. */
const DEFAULT_EFFORT = "default";

/**
 * The effort control a chat shows. Its session's own effort option once it
 * has one; before the chat's first session, a model whose thinking levels
 * the harness lists takes a level here, held as a draft that goes with the
 * session start.
 */
export function useDraftEffortOption({
  effortOption,
  hasSessionConfig,
  draftChatId,
  bindingId,
  modelId,
  models,
  onSetSessionConfigOption,
}: {
  effortOption: AcpConfigOption | undefined;
  hasSessionConfig: boolean;
  draftChatId: string | undefined;
  bindingId: string | null;
  modelId: string | null;
  models: readonly ModelOption[];
  onSetSessionConfigOption: (
    configId: string,
    value: AcpConfigValue,
  ) => Promise<unknown>;
}) {
  const held = useChatEffortDrafts((state) =>
    draftChatId ? state.drafts[draftChatId] : undefined,
  );
  const levels =
    !effortOption && !hasSessionConfig && draftChatId && bindingId && modelId
      ? models.find((model) => model.id === modelId)?.effortLevels
      : undefined;
  const heldLevel =
    held?.bindingId === bindingId && held.modelId === modelId
      ? held.effort
      : null;
  // A level the model no longer lists (a new Claude Code build) is dropped,
  // so the start never carries a level the control does not show.
  const stale =
    !!draftChatId &&
    !!levels &&
    heldLevel !== null &&
    heldLevel !== DEFAULT_EFFORT &&
    !levels.includes(heldLevel);
  useEffect(() => {
    if (stale && draftChatId) draftChatEffort(draftChatId, null);
  }, [stale, draftChatId]);
  const draftOption: AcpConfigOption | undefined = levels?.length
    ? {
        id: DRAFT_EFFORT_ID,
        name: "Thinking level",
        category: "thought_level",
        type: "select",
        currentValue: heldLevel && !stale ? heldLevel : DEFAULT_EFFORT,
        options: [DEFAULT_EFFORT, ...levels].map((level) => ({
          value: level,
          name: capitalizeLabel(level),
        })),
      }
    : undefined;
  const setConfigOption = (configId: string, value: AcpConfigValue) => {
    if (
      configId === DRAFT_EFFORT_ID &&
      draftChatId &&
      bindingId &&
      modelId &&
      "value" in value &&
      typeof value.value === "string"
    ) {
      draftChatEffort(draftChatId, { bindingId, modelId, effort: value.value });
      return Promise.resolve();
    }
    return onSetSessionConfigOption(configId, value);
  };
  return { shownEffortOption: effortOption ?? draftOption, setConfigOption };
}
