import { Fragment, useId, useMemo } from "react";
import { PickerOption } from "../../ui/PickerOption";
import type { SearchablePickerOption } from "../../ui/SearchablePicker";
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
  // Build option data once per catalog: a fresh candidate object on every
  // render would re-render each memoised option on any focus change.
  const groups = useMemo(() => {
    const visible = models.slice(0, MAX_VISIBLE_MODELS);
    const names = visible.map((model) => composerModelName(model.name));
    const recommended = visible.some((model) => model.isDefault);
    const grouped = new Map<string, SearchablePickerOption[]>();
    visible.forEach((model, index) => {
      const group =
        model.group ??
        (recommended ? (model.isDefault ? "Recommended" : "Other models") : "");
      const label = names[index]!;
      const duplicate = names.indexOf(label) < names.lastIndexOf(label);
      grouped.set(group, [
        ...(grouped.get(group) ?? []),
        {
          id: model.id,
          label,
          subtitle: duplicate ? model.id : undefined,
          accessibleLabel: duplicate ? `${label} (${model.id})` : undefined,
        },
      ]);
    });
    return grouped;
  }, [models]);
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
          const options = choices.map((candidate) => (
            <PickerOption
              key={candidate.id}
              id={`${id}-${encodeURIComponent(candidate.id)}`}
              candidate={candidate}
              active={false}
              selected={candidate.id === selectedId}
              showSelectedCheck
              disabled={disabled}
              onSelect={onSelect}
            />
          ));
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
