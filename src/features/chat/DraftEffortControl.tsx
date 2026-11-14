import type { ComponentProps } from "react";
import { useChatEffortDraft } from "../../app/chatEffortDraft";
import { capitalizeLabel } from "../../domain/text";
import { ComposerEffortControl } from "./ComposerEffortControl";

export interface DraftEffort {
  chatId: string;
  bindingId: string;
  modelId: string;
  levels: readonly string[];
}

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
  const current =
    held?.chatId === chatId &&
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
        if ("value" in value && typeof value.value === "string")
          useChatEffortDraft.setState({
            draft: { chatId, bindingId, modelId, effort: value.value },
          });
        return Promise.resolve();
      }}
    />
  );
}
