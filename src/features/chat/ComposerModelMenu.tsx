import type { AcpConfigOption } from "../../domain/acpSessionConfig";
import { useComposerModelChoices } from "./useComposerModelChoices";
import type { ReactNode, RefObject } from "react";
import { MenuPopover } from "../../ui/MenuPopover";
import "./composer-model-identities.css";
import {
  ChatModelPicker,
  MAX_VISIBLE_MODELS,
  type ComposerModelChoice,
} from "./ChatModelPicker";

/**
 * The model popover is a dialog: the model list keeps one tab stop on its
 * selected model, and Tab moves on to the thinking-level section below it.
 */
export default function ComposerModelMenu({
  triggerRef,
  panelId,
  choices: source,
  selectedId,
  contextWindow,
  disabled,
  onDismiss,
  onSelect,
  children,
}: {
  triggerRef: RefObject<HTMLButtonElement | null>;
  panelId: string;
  choices:
    | readonly ComposerModelChoice[]
    | Extract<AcpConfigOption, { type: "select" }>;
  selectedId: string | null;
  contextWindow?: string | null;
  disabled: boolean;
  onDismiss: () => void;
  onSelect: (modelId: string) => void;
  children?: ReactNode;
}) {
  const choices = useComposerModelChoices(source);
  return (
    <MenuPopover
      open
      onDismiss={onDismiss}
      triggerRef={triggerRef}
      focusKey={choices
        .slice(0, MAX_VISIBLE_MODELS)
        .map((model) => model.id)
        .join("\0")}
      id={panelId + "-popover"}
      role="dialog"
      className="composer__popover composer__model-menu composer-model-picker"
      ariaLabel="Select model"
    >
      {/* Without listed models the popover holds only the thinking level. */}
      {choices.length > 0 ? (
        <ChatModelPicker
          listboxId={panelId}
          models={choices}
          selectedId={selectedId}
          disabled={disabled}
          onSelect={onSelect}
        />
      ) : null}
      {contextWindow ? (
        <span className="composer__capacity">{contextWindow} context</span>
      ) : null}
      {children}
    </MenuPopover>
  );
}
