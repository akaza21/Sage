import type { FormEvent, ReactNode, RefObject } from "react";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type { AcpGoalState } from "../../domain/acpGoal";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { AttachedContext } from "../../domain/attachedContext";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";
import type { ChatUsage } from "./acp/chatUsage";

export interface ChatComposerRegionProps {
  /** Kept mounted while an approval holds the slot, so the draft survives. */
  hidden?: boolean;
  approvalSlot?: ReactNode;
  onEditSummaryMount?: (element: HTMLDivElement | null) => void;
  composerRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  dictationScope: string;
  /** The bound Chat whose live session lists the `/` commands. */
  commandChatId?: string;
  sessionId?: string | null;
  descriptionId: string;
  disabledReasonId: string;
  text: string;
  attachedContext: readonly AttachedContext[];
  imagePreviews?: Readonly<Record<string, string>>;
  imageUploading?: boolean;
  enabled: boolean;
  sendBlocked?: boolean;
  actionBusy: boolean;
  activeTurn: boolean;
  queueAvailable?: boolean;
  queuedPrompts?: readonly QueuedChatPrompt[];
  steerAvailable?: boolean;
  projectTransitioning: boolean;
  workbenchError: string | null;
  chatError: string | null;
  claudeSignInRequired?: boolean;
  claudeAuthScope?: string;
  accessBindingId?: string | null;
  availableBinding: boolean;
  canPrompt: boolean;
  discoveryPhase: HarnessDiscoveryPhase;
  sessionHarnessId: string | null;
  sessionModelId: string | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  sessionPromptUnavailable: boolean;
  sessionConfig: AcpSessionConfig | null;
  modePending: boolean;
  modeError: string | null;
  focused: boolean;
  onSetSessionMode: (modeId: string) => Promise<unknown>;
  onSetSessionConfigOption: (
    configId: string,
    value: AcpConfigValue,
  ) => Promise<unknown>;
  onSelectHarness: (harnessId: string) => Promise<void>;
  onSelectModel: (modelId: string) => void;
  missingBindingNotice: boolean;
  disabledReason: string | null;
  onRetryLoad?: (() => void) | null;
  project: { name: string } | null | undefined;
  usage: ChatUsage | null;
  goal: AcpGoalState;
  /** pickedCommand: the `/name` the user picked from the command menu. */
  onSubmit: (
    event: FormEvent<HTMLFormElement>,
    options?: { pickedCommand?: string },
  ) => void;
  onTextChange: (text: string) => void;
  onRemoveContext: (context: AttachedContext) => void;
  imageInputAvailable?: boolean;
  onAddImages?: (files: File[]) => void;
  onCancelTurn: (() => void) | null;
  onDeleteQueuedPrompt?: (id: string) => void;
  onRetryQueuedPrompt?: (id: string) => void;
  onSteerQueuedPrompt?: (id: string) => void;
}
