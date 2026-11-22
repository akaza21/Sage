import { Check } from "../../ui/leanIcons";
import { Lightbulb, Paperclip, Target } from "../../ui/leanIconNodesLazy";
import {
  useContext,
  useLayoutEffect,
  useRef,
  type ComponentProps,
  type ReactNode,
  type RefObject,
} from "react";
import { MenuPopover } from "../../ui/MenuPopover";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import { ChatSessionConfigControls } from "./ChatSessionConfigControls";
import { attachRefusal } from "./composerAttachRefusal";
import {
  sessionConfigControls,
  sessionModeActions,
  flattenChoices,
} from "../../domain/acpSessionConfigControls";
import { withoutGovernedModes } from "../../domain/governedSessionModes";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import { AgentToolsContext } from "../../workbench/AgentToolsProvider";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import {
  ComposerPlanAccess,
  type PlanAccessTarget,
} from "./ComposerPlanAccess";

export { ListedHarnessModels } from "./ListedHarnessModels";
export { ComposerNoticeSurface, HarnessAccountMenu } from "./ChatSignInNotices";

export type ComposerMenuActions = Pick<
  ComponentProps<typeof ComposerAddActions>,
  | "onAttachFiles"
  | "attachRefusalChatId"
  | "onOpenGoal"
  | "onClearGoal"
  | "goalUnobserved"
  | "onCompact"
>;

export function ComposerOptionsMenu({
  id,
  triggerRef,
  onDismiss,
  selectionKey,
  canSelectEffort,
  focused,
  onModeRemoved,
  accessModes,
  ...given
}: ComponentProps<typeof ComposerAddActions> & {
  accessModes?: readonly SessionAccessModeCapability[];
  id: string;
  selectionKey: string;
  canSelectEffort: boolean;
  focused: boolean;
  onModeRemoved: () => void;
  triggerRef: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
}) {
  const actions = {
    ...given,
    sessionConfig: withoutGovernedModes(given.sessionConfig, accessModes),
  };
  const actionsKey = JSON.stringify([
    selectionKey,
    actions.sessionConfig?.sageAgentSessionId,
    !!actions.onAttachFiles,
    !!actions.planAccess,
    !!actions.onOpenGoal,
    !!actions.onClearGoal,
    actions.goalUnobserved,
    !!actions.onCompact,
    canSelectEffort,
    sessionModeActions(actions.sessionConfig).map((mode) => mode.id),
    sessionConfigControls(actions.sessionConfig, "other").map((option) => [
      option.id,
      option.type,
      flattenChoices(option).map((choice) => choice.value),
    ]),
  ]);
  const previousActionsKey = useRef(actionsKey);
  useLayoutEffect(() => {
    if (previousActionsKey.current !== actionsKey) {
      const element = document.activeElement;
      const menuHadFocus =
        element === document.body ||
        document.getElementById(id)?.contains(element);
      onDismiss();
      if (focused && menuHadFocus) onModeRemoved();
    }
    previousActionsKey.current = actionsKey;
  }, [actionsKey, focused, id, onDismiss, onModeRemoved]);
  return (
    <MenuPopover
      id={id}
      className="composer__popover composer__options"
      ariaLabel="Composer options menu"
      initialFocus="selected"
      focusKey="options"
      restoreFocusOnTab
      open
      triggerRef={triggerRef}
      onDismiss={onDismiss}
      gap={8}
    >
      <ComposerAddActions {...actions} />
    </MenuPopover>
  );
}

export function ComposerAddActions({
  sessionConfig,
  projectAction,
  planAccess,
  pending,
  disabled,
  onAttachFiles,
  attachRefusalChatId,
  onOpenGoal,
  onClearGoal,
  goalUnobserved,
  harnessName = "This harness",
  onCompact,
  onOpenEffort,
  onClose,
  onSetSessionMode,
  onSetSessionConfigOption,
}: {
  sessionConfig: AcpSessionConfig | null;
  projectAction?: ReactNode;
  planAccess?: PlanAccessTarget;
  pending: boolean;
  disabled: boolean;
  onAttachFiles?: () => void;
  attachRefusalChatId?: string;
  onOpenGoal?: () => void;
  onClearGoal?: () => void;
  goalUnobserved?: boolean;
  harnessName?: string;
  onCompact?: () => void;
  onOpenEffort?: () => void;
  onClose: () => void;
  onSetSessionMode: (id: string) => Promise<unknown>;
  onSetSessionConfigOption: (
    id: string,
    value: AcpConfigValue,
  ) => Promise<unknown>;
}) {
  const modes = sessionModeActions(sessionConfig);
  const attachRefused =
    onAttachFiles && attachRefusalChatId !== undefined
      ? attachRefusal(attachRefusalChatId)
      : null;
  const { launch, chatAvailable } = useContext(AgentToolsContext);
  const importable = useStore(
    acpProjectionStoreFor(usePinnedAgentChatOwner()),
    ({ page }) =>
      page?.sessions.some(
        (session) =>
          session.id === page.currentSessionId &&
          session.capabilities.nativeSessionImport,
      ),
  );
  const sessionAction = (
    label: string,
    action: () => void,
    description?: string,
    icon?: ReactNode,
  ) => (
    <button
      type="button"
      role="menuitem"
      className="menu-item composer__add-action"
      aria-label={label}
      aria-description={description}
      disabled={disabled}
      tabIndex={-1}
      onClick={() => {
        onClose();
        action();
      }}
    >
      {icon}
      <span>{label}</span>
      {description ? (
        <span className="composer__action-description">{description}</span>
      ) : null}
    </button>
  );
  const goalItem = onOpenGoal
    ? sessionAction(
        goalUnobserved ? "Set goal\u2026" : "Goal",
        onOpenGoal,
        "Set a goal to keep pursuing",
        <Target size={14} aria-hidden="true" />,
      )
    : null;
  return (
    <>
      <p className="menu-group-label composer__add-heading" aria-hidden="true">
        Add
      </p>
      {onAttachFiles ? (
        <button
          type="button"
          role="menuitem"
          className="menu-item composer__add-action"
          aria-label="Files and folders"
          aria-description={attachRefused ?? undefined}
          aria-disabled={attachRefused !== null}
          tabIndex={-1}
          onClick={() => {
            if (attachRefused !== null) return;
            onClose();
            onAttachFiles();
          }}
        >
          <Paperclip size={14} aria-hidden="true" />
          <span>Files and folders</span>
          {attachRefused ? (
            <span className="composer__action-description">
              {attachRefused}
            </span>
          ) : null}
        </button>
      ) : null}
      {projectAction}
      {!goalUnobserved ? goalItem : null}
      {planAccess ? (
        <ComposerPlanAccess
          target={planAccess}
          disabled={disabled}
          onClose={onClose}
        />
      ) : null}
      {modes.length ? (
        <div
          role="group"
          aria-label="Session mode"
          aria-description={sessionConfig?.restored}
          title={sessionConfig?.restored}
          data-restored={sessionConfig?.restored ? true : undefined}
        >
          {modes.map((mode) => {
            const plan = mode.name.toLocaleLowerCase() === "plan";
            const label = plan ? "Plan mode" : mode.name || mode.id;
            const selected = mode.id === sessionConfig?.modes?.currentModeId;
            const description = plan
              ? selected
                ? "Plan mode is on"
                : "Turn plan mode on"
              : mode.description || undefined;
            return (
              <button
                key={mode.id}
                type="button"
                role="menuitemradio"
                className="menu-item composer__add-action"
                aria-label={label}
                aria-description={description}
                aria-checked={selected}
                disabled={disabled || pending}
                tabIndex={-1}
                onClick={() => {
                  onClose();
                  if (!selected) void onSetSessionMode(mode.id);
                }}
              >
                {plan ? <Lightbulb size={14} aria-hidden="true" /> : null}
                <span>{label}</span>
                {description ? (
                  <span className="composer__action-description">
                    {description}
                  </span>
                ) : null}
                {selected ? <Check size={14} aria-hidden="true" /> : null}
              </button>
            );
          })}
        </div>
      ) : null}
      {goalUnobserved && (onOpenGoal || onClearGoal) ? (
        <p
          className="menu-group-label composer__add-heading"
          aria-hidden="true"
        >
          Session
        </p>
      ) : null}
      {goalUnobserved ? goalItem : null}
      {onClearGoal ? sessionAction("Clear goal", onClearGoal) : null}
      {goalUnobserved && (onOpenGoal || onClearGoal) ? (
        <p className="composer__action-description">
          {harnessName} doesn't report goal progress to Sage.
        </p>
      ) : null}
      {onCompact
        ? sessionAction(
            "Compact context",
            onCompact,
            "Summarize earlier turns to free context",
          )
        : null}
      {onOpenEffort ? (
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          disabled={disabled}
          tabIndex={-1}
          onClick={onOpenEffort}
          data-overflow-control="effort"
        >
          Thinking level
        </button>
      ) : null}
      {chatAvailable && importable ? (
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          tabIndex={-1}
          onClick={() => {
            onClose();
            launch("sessionImport");
          }}
        >
          Open a Codex CLI session
        </button>
      ) : null}
      <ChatSessionConfigControls
        sessionConfig={sessionConfig}
        group="other"
        pending={pending}
        disabled={disabled}
        onSetSessionConfigOption={(id, value) => {
          onClose();
          return onSetSessionConfigOption(id, value);
        }}
      />
    </>
  );
}
