import type { ComponentProps } from "react";
import {
  DEFAULT_EFFORT_DRAFT,
  useChatEffortDraft,
} from "../../app/chatEffortDraft";
import { capitalizeLabel } from "../../domain/text";
import { ComposerEffortControl } from "./ComposerEffortControl";

/** The thinking levels a chat may start its first session with. */
export interface DraftEffort {
  chatId: string;
  bindingId: string;
  modelId: string;
  levels: readonly string[];
}

/**
 * The Thinking level control before a chat's first session: the levels the
 * harness lists for the selected model, held as a draft that goes with the
 * session start. A held level the model no longer lists still shows as
 * held: the start refuses it rather than running Default unseen, so it is
 * changed here.
 */
export default function DraftEffortControl({
  draft,
  ...props
}: { draft: DraftEffort } & Omit<
  ComponentProps<typeof ComposerEffortControl>,
  "option" | "onSetOption"
>) {
  const held = useChatEffortDraft((state) => state.draft);
  const { chatId, bindingId, modelId } = draft;
  const current =
    held?.chatId === chatId &&
    held.bindingId === bindingId &&
    held.modelId === modelId
      ? held.effort
      : DEFAULT_EFFORT_DRAFT;
  const listed = [DEFAULT_EFFORT_DRAFT, ...draft.levels];
  const values = listed.includes(current) ? listed : [...listed, current];
  return (
    <ComposerEffortControl
      {...props}
      option={{
        id: "effort",
        name: "Thinking level",
        category: "thought_level",
        type: "select",
        currentValue: current,
        options: values.map((value) => ({
          value,
          name: capitalizeLabel(value),
        })),
      }}
      onSetOption={(_, value) => {
        if ("value" in value && typeof value.value === "string")
          useChatEffortDraft.setState({
            draft: { chatId, bindingId, modelId, effort: value.value },
          });
        return Promise.resolve();
      }}
    />
  );
}
