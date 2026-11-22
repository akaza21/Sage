import type { ComponentProps, ReactNode, RefObject } from "react";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import type { PlanAccessTarget } from "./ComposerPlanAccess";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";
import type { ImageAction } from "./useComposerImageInput";
import type { ControlledDraftPickers } from "./chatHarnessControlsProps";
import { lazyParts } from "./lazyParts";
import type * as SectionChunk from "./ComposerSectionMenu";
const SectionPart = lazyParts<
  Pick<typeof SectionChunk, "ComposerOptionsMenu" | "ComposerSectionMenu">
>(() => import("./ComposerSectionMenu"));

export { ListedHarnessModels } from "./ListedHarnessModels";
export { ComposerNoticeSurface, HarnessAccountMenu } from "./ChatSignInNotices";

export type ComposerMenuActions = Pick<
  ComponentProps<typeof ComposerAddActions>,
  | "onAttachFiles"
  | "imageAction"
  | "boundChatId"
  | "attachRefusalChatId"
  | "onOpenGoal"
  | "onClearGoal"
  | "goalUnobserved"
  | "onCompact"
>;

export type ComposerOptionsProps = ComponentProps<typeof ComposerAddActions> & {
  accessModes?: readonly SessionAccessModeCapability[];
  id: string;
  selectionKey: string;
  canSelectEffort: boolean;
  focused: boolean;
  onModeRemoved: () => void;
  triggerRef: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
};

export function ComposerOptionsMenu(props: ComposerOptionsProps) {
  return <SectionPart part="ComposerOptionsMenu" {...props} />;
}
export type ComposerAddActionsProps = {
  boundChatId?: string;
  /** The new-chat draft's pickers. Only rows needing no session are offered. */
  draftPickers?: ControlledDraftPickers;
  sessionConfig: AcpSessionConfig | null;
  projectAction?: ReactNode;
  planAccess?: PlanAccessTarget;
  pending: boolean;
  disabled: boolean;
  onAttachFiles?: () => void;
  imageAction?: ImageAction;
  attachRefusalChatId?: string;
  onOpenGoal?: () => void;
  onClearGoal?: () => void;
  goalUnobserved?: boolean;
  harnessName?: string;
  onCompact?: () => void;
  onOpenEffort?: () => void;
  onOpenUsage?: () => void;
  onOpenMcp?: () => void;
  onPickCommand?: (command: AvailableAcpCommand, input: string) => void;
  onClose: () => void;
  onSetSessionMode: (id: string) => Promise<unknown>;
  onSetSessionConfigOption: (
    id: string,
    value: AcpConfigValue,
  ) => Promise<unknown>;
};

export function ComposerAddActions(props: ComposerAddActionsProps) {
  return <SectionPart part="ComposerSectionMenu" {...props} />;
}
