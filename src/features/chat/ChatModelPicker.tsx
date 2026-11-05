import { Fragment, useId } from "react";
import { PickerOption } from "../../ui/PickerOption";
import { composerModelName } from "./composerModelName";

export interface ComposerModelChoice {
  id: string;
  name: string;
  group?: string;
  isDefault?: boolean;
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
  const visible = models.slice(0, MAX_VISIBLE_MODELS);
  const names = visible.map((model) => composerModelName(model.name));
  const nameCounts = new Map<string, number>();
  for (const name of names)
    nameCounts.set(name, (nameCounts.get(name) ?? 0) + 1);
  const recommended = visible.some((model) => model.isDefault);
  const groups = new Map<string, ComposerModelChoice[]>();
  for (const model of visible) {
    const group =
      model.group ??
      (recommended ? (model.isDefault ? "Recommended" : "Other models") : "");
    const choices = groups.get(group) ?? [];
    choices.push(model);
    groups.set(group, choices);
  }
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
        {Array.from(groups, ([group, choices]) => {
          const options = choices.map((model) => {
            const label = composerModelName(model.name);
            const duplicate = (nameCounts.get(label) ?? 0) > 1;
            return (
              <PickerOption
                key={model.id}
                id={`${id}-${encodeURIComponent(model.id)}`}
                candidate={{
                  id: model.id,
                  label,
                  subtitle: duplicate ? model.id : undefined,
                  accessibleLabel: duplicate
                    ? `${label} (${model.id})`
                    : undefined,
                }}
                active={false}
                selected={model.id === selectedId}
                showSelectedCheck
                disabled={disabled}
                onSelect={onSelect}
              />
            );
          });
          return group ? (
            <div key={group} role="group" aria-label={group}>
              <p className="menu-group-label" aria-hidden="true">
                {group}
              </p>
              {options}
            </div>
          ) : (
            <Fragment key={group}>{options}</Fragment>
          );
        })}
      </div>
      {models.length > MAX_VISIBLE_MODELS ? (
        <p className="menu-empty" role="status">
          Showing first {MAX_VISIBLE_MODELS} models
        </p>
      ) : null}
    </>
  );
}
