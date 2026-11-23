import type { ComponentProps } from "react";
import {
  chatEffortField,
  DEFAULT_EFFORT_DRAFT,
  useChatEffortDraft,
} from "../../app/chatEffortDraft";
import { capitalizeLabel } from "../../domain/text";
import { ComposerEffortControl } from "./ComposerEffortControl";

export interface DraftEffort {
  chatId?: string;
  value?: string | null;
  onChange?: (value: string) => void;
  bindingId: string;
  modelId: string;
  levels: readonly string[];
}

/**
 * A held level the model no longer lists still shows as held: the start
 * refuses it rather than running Default unseen, so it is changed here.
 */
export default function DraftEffortControl({
  draft,
  ...props
}: { draft: DraftEffort } & Omit<
  ComponentProps<typeof ComposerEffortControl>,
  "option" | "onSetOption"
>) {
  // Subscribed so a held level re-renders; the field reads the same draft.
  useChatEffortDraft((state) => state.draft);
  const { chatId, bindingId, modelId } = draft;
  const current =
    (draft.onChange
      ? draft.value
      : chatId && chatEffortField(chatId, bindingId, modelId).effort) ||
    DEFAULT_EFFORT_DRAFT;
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
        if ("value" in value && typeof value.value === "string") {
          if (draft.onChange) draft.onChange(value.value);
          else if (chatId)
            useChatEffortDraft.setState({
              draft: { chatId, bindingId, modelId, effort: value.value },
            });
        }
        return Promise.resolve();
      }}
    />
  );
}
