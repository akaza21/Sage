import type { AcpConfigOption } from "../../domain/acpSessionConfig";
import { useComposerModelChoices } from "./useComposerModelChoices";
import type { RefObject } from "react";
import { MenuPopover } from "../../ui/MenuPopover";
import "./composer-model-identities.css";
import {
  ChatModelPicker,
  MAX_VISIBLE_MODELS,
  type ComposerModelChoice,
} from "./ChatModelPicker";

export default function ComposerModelMenu({
  triggerRef,
  panelId,
  choices: source,
  selectedId,
  disabled,
  onDismiss,
  onSelect,
}: {
  triggerRef: RefObject<HTMLButtonElement | null>;
  panelId: string;
  choices:
    | readonly ComposerModelChoice[]
    | Extract<AcpConfigOption, { type: "select" }>;
  selectedId: string | null;
  disabled: boolean;
  onDismiss: () => void;
  onSelect: (modelId: string) => void;
}) {
  const choices = useComposerModelChoices(source);
  return (
    <MenuPopover
      open
      onDismiss={onDismiss}
      restoreFocusOnTab
      triggerRef={triggerRef}
      initialFocus="selected"
      focusKey={choices
        .slice(0, MAX_VISIBLE_MODELS)
        .map((model) => model.id)
        .join("\0")}
      id={panelId + "-popover"}
      role="none"
      className="composer__popover composer__model-menu composer-model-picker"
      ariaLabel="Select model"
    >
      <ChatModelPicker
        listboxId={panelId}
        models={choices}
        selectedId={selectedId}
        disabled={disabled}
        onSelect={onSelect}
      />
    </MenuPopover>
  );
}
