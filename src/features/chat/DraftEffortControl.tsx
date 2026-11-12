import type { ComponentProps } from "react";
import { useChatEffortDraft } from "../../app/chatEffortDraft";
import { capitalizeLabel } from "../../domain/text";
import { ComposerEffortControl } from "./ComposerEffortControl";

/** The thinking levels a chat may start its first session with. */
export interface DraftEffort {
  chatId?: string;
  value?: string | null;
  onChange?: (value: string) => void;
  bindingId: string;
  modelId: string;
  levels: readonly string[];
}

/**
 * The Thinking level control before a chat's first session: the levels the
 * harness lists for the selected model, held as a draft that goes with the
 * session start. A held level the model no longer lists shows, and is
 * started, as Default.
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
  const values = ["default", ...draft.levels];
  const current = draft.onChange
    ? (draft.value ?? "default")
    : held &&
        held.chatId === chatId &&
        held.bindingId === bindingId &&
        held.modelId === modelId &&
        values.includes(held.effort)
      ? held.effort
      : "default";
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
