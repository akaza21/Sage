import type { ReactNode } from "react";
import { Check } from "../../ui/leanIcons";
import { Lightbulb, Paperclip, Target } from "../../ui/leanIconNodesLazy";
import {
  flattenChoices,
  sessionConfigControls,
  sessionModeActions,
} from "../../domain/acpSessionConfigControls";
import { attachRefusal } from "./composerAttachRefusal";
import { ComposerPlanAccess } from "./ComposerPlanAccess";
import type { ComposerAddActionsProps } from "./ComposerAddActions";
import type { ComposerNativeRow } from "./composerMenuSections";

export function composerNativeRows(
  props: ComposerAddActionsProps & {
    importable: boolean;
    onImport: () => void;
  },
): ComposerNativeRow[] {
  const { sessionConfig, disabled, pending, onClose } = props;
  const rows: ComposerNativeRow[] = [];
  const add = (
    id: string,
    section: ComposerNativeRow["section"],
    label: string,
    content: ReactNode,
    description?: string,
    control?: ComposerNativeRow["control"],
    requires?: ComposerNativeRow["requires"],
  ) => {
    rows.push({
      kind: "native",
      id,
      section,
      label,
      description,
      control,
      requires,
      content,
    });
  };
  const action = (
    id: string,
    section: ComposerNativeRow["section"],
    label: string,
    run: (() => void) | undefined,
    description?: string,
    icon?: ReactNode,
    control?: ComposerNativeRow["control"],
    closeMenu = true,
  ) => {
    if (!run) return;
    add(
      id,
      section,
      label,
      <button
        type="button"
        role="menuitem"
        className="menu-item composer__add-action"
        aria-label={label}
        aria-description={description}
        disabled={disabled}
        tabIndex={-1}
        onClick={() => {
          if (closeMenu) onClose();
          run();
        }}
      >
        {icon}
        <span>{label}</span>
        {description ? (
          <span className="composer__action-description">{description}</span>
        ) : null}
      </button>,
      description,
      control,
    );
  };
  if (props.onAttachFiles) {
    const refusal =
      props.attachRefusalChatId !== undefined
        ? attachRefusal(props.attachRefusalChatId)
        : null;
    add(
      "files",
      "addToMessage",
      "Files and folders",
      <button
        type="button"
        role="menuitem"
        className="menu-item composer__add-action"
        aria-label="Files and folders"
        aria-description={refusal ?? undefined}
        aria-disabled={refusal !== null}
        tabIndex={-1}
        onClick={() => {
          if (refusal !== null) return;
          onClose();
          props.onAttachFiles?.();
        }}
      >
        <Paperclip size={14} aria-hidden="true" />
        <span>Files and folders</span>
        {refusal ? (
          <span className="composer__action-description">{refusal}</span>
        ) : null}
      </button>,
      refusal ?? undefined,
    );
  }
  const imageAction = props.imageAction;
  if (imageAction && !imageAction.reason)
    add(
      "images",
      "addToMessage",
      "Add images",
      <button
        type="button"
        role="menuitem"
        className="menu-item composer__add-action"
        disabled={disabled || imageAction.disabled}
        tabIndex={-1}
        onClick={() => {
          onClose();
          imageAction.onChoose();
        }}
      >
        <span>Add images</span>
      </button>,
      undefined,
      undefined,
      "none",
    );
  if (props.projectAction)
    add("project", "addToMessage", "Work in a project", props.projectAction);
  action(
    "goal",
    "session",
    props.goalUnobserved ? "Set goal…" : "Goal",
    props.onOpenGoal,
    props.goalUnobserved
      ? `${props.harnessName ?? "This harness"} doesn't report goal progress to Sage.`
      : "Set a goal to keep pursuing",
    <Target size={14} aria-hidden="true" />,
    "goalEditor",
  );
  action("clearGoal", "session", "Clear goal", props.onClearGoal);
  action(
    "compact",
    "session",
    "Compact context",
    props.onCompact,
    "Summarize earlier turns to free context",
    undefined,
    "compact",
  );
  if (props.importable)
    action("import", "session", "Open a Codex CLI session", props.onImport);
  action(
    "mcp",
    "connections",
    "MCP servers",
    props.onOpenMcp,
    undefined,
    undefined,
    "mcpPanel",
    false,
  );
  action(
    "extensions",
    "addOns",
    "Extensions",
    props.onOpenExtensions,
    undefined,
    undefined,
    undefined,
    false,
  );
  if (props.planAccess)
    add(
      "plan",
      "modelAndAccess",
      "Plan mode",
      <ComposerPlanAccess
        // The draft's own access picker state, never a second copy.
        target={{
          ...props.planAccess,
          value: props.draftPickers?.access,
          onChange: props.draftPickers?.onAccessChange,
        }}
        disabled={disabled}
        onClose={onClose}
      />,
      undefined,
      undefined,
      "none",
    );
  // Harness modes and options share the action rows' label, description
  // and check columns.
  const choiceRow = (
    id: string,
    section: ComposerNativeRow["section"],
    label: string,
    checked: boolean,
    run: () => void,
    text = label,
    description?: string,
    icon?: ReactNode,
    role = "menuitemradio",
  ) =>
    add(
      id,
      section,
      label,
      <button
        type="button"
        role={role}
        className="menu-item composer__add-action"
        aria-label={label}
        aria-description={
          [description, sessionConfig?.restored].filter(Boolean).join(" ") ||
          undefined
        }
        aria-checked={checked}
        disabled={disabled || pending}
        tabIndex={-1}
        title={sessionConfig?.restored}
        data-restored={sessionConfig?.restored ? true : undefined}
        onClick={() => {
          onClose();
          run();
        }}
      >
        {icon}
        <span>{text}</span>
        {description ? (
          <span className="composer__action-description">{description}</span>
        ) : null}
        {checked ? <Check size={14} aria-hidden="true" /> : null}
      </button>,
      description,
    );
  for (const mode of sessionModeActions(sessionConfig)) {
    const plan = mode.name.toLocaleLowerCase() === "plan";
    const label = plan ? "Plan mode" : mode.name || mode.id;
    const selected = mode.id === sessionConfig?.modes?.currentModeId;
    choiceRow(
      `mode:${mode.id}`,
      "modelAndAccess",
      label,
      selected,
      () => {
        if (!selected) void props.onSetSessionMode(mode.id);
      },
      label,
      plan
        ? selected
          ? "Plan mode is on"
          : "Turn plan mode on"
        : mode.description || undefined,
      plan ? <Lightbulb size={14} aria-hidden="true" /> : null,
    );
  }
  // This action switches the toolbar's active popover itself.
  if (props.onOpenEffort)
    add(
      "effort",
      "modelAndAccess",
      "Thinking level",
      <button
        type="button"
        role="menuitem"
        className="menu-item"
        disabled={disabled}
        tabIndex={-1}
        onClick={props.onOpenEffort}
        data-overflow-control="effort"
      >
        Thinking level
      </button>,
      undefined,
      "effortPicker",
    );
  for (const option of sessionConfigControls(sessionConfig, "other")) {
    if (option.type === "boolean") {
      choiceRow(
        `config:${option.id}`,
        "session",
        `Session ${option.name.toLocaleLowerCase()}`,
        option.currentValue,
        () =>
          void props.onSetSessionConfigOption(option.id, {
            type: "boolean",
            value: !option.currentValue,
          }),
        option.name,
        undefined,
        undefined,
        "menuitemcheckbox",
      );
    } else if (option.type === "select") {
      for (const choice of flattenChoices(option))
        choiceRow(
          `config:${option.id}:${choice.value}`,
          "session",
          `${option.name}: ${choice.name || choice.value}`,
          choice.value === option.currentValue,
          () =>
            void props.onSetSessionConfigOption(option.id, {
              value: choice.value,
            }),
        );
    } else {
      add(
        `config:${option.id}`,
        "session",
        option.name,
        <span className="menu-empty" aria-disabled="true">
          {option.name}
        </span>,
      );
    }
  }
  action(
    "usage",
    "usage",
    "Usage",
    props.onOpenUsage,
    undefined,
    undefined,
    "usagePanel",
    false,
  );
  return rows;
}
