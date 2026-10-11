import { useId } from "react";
import { PickerOption } from "../../ui/PickerOption";

export interface ComposerModelChoice {
  id: string;
  name: string;
}
export const MAX_VISIBLE_MODELS = 200;

export function ChatModelPicker({
  listboxId,
  models,
  selectedId,
  disabled,
  onSelect,
}: {
  listboxId: string;
  models: readonly ComposerModelChoice[];
  selectedId: string | null;
  disabled: boolean;
  onSelect: (modelId: string) => void;
}) {
  const id = useId();
  return (
    <>
      <p id={`${id}-title`} className="composer-model-picker__title">
        Select model
      </p>
      <div
        id={listboxId}
        className="composer-model-picker__list"
        role="listbox"
        aria-labelledby={`${id}-title`}
      >
        {models.slice(0, MAX_VISIBLE_MODELS).map((model) => (
          <PickerOption
            key={model.id}
            id={`${id}-${encodeURIComponent(model.id)}`}
            candidate={{ id: model.id, label: model.name }}
            active={false}
            selected={model.id === selectedId}
            showSelectedCheck
            disabled={disabled}
            onSelect={onSelect}
          />
        ))}
      </div>
      {models.length > MAX_VISIBLE_MODELS ? (
        <p className="menu-empty" role="status">
          Showing first {MAX_VISIBLE_MODELS} models
        </p>
      ) : null}
    </>
  );
}
