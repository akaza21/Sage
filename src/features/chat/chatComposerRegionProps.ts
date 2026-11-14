import type { FormEvent, ReactNode, RefObject } from "react";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type { AcpGoalState } from "../../domain/acpGoal";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { AttachedContext } from "../../domain/attachedContext";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";
import type { HarnessDefinition } from "../../domain/harness";
import type { ChatUsage } from "./acp/chatUsage";
import type { PasteAttachability } from "./composerPasteText";
import type { AcpUsageRequest } from "../../domain/acpUsage";

export interface ComposerAccessTarget {
  bindingId?: string | null;
  draftChatId?: string;
}

export interface ChatComposerRegionProps {
  hidden?: boolean;
  approvalSlot?: ReactNode;
  onEditSummaryMount?: (element: HTMLDivElement | null) => void;
  composerRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  dictationScope: string;
  commandChatId?: string;
  sessionId?: string | null;
  descriptionId: string;
  disabledReasonId: string;
  text: string;
  attachedContext: readonly AttachedContext[];
  pasteSession?: PasteAttachability["session"];
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
  signInHarness?: Pick<HarnessDefinition, "name" | "signIn"> | null;
  claudeSignInRequired?: boolean;
  claudeAuthScope?: string;
  accessTarget?: ComposerAccessTarget;
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
  onCompact?: () => void;
  onSelectModel: (modelId: string) => void;
  missingBindingNotice: boolean;
  disabledReason: string | null;
  onRetryLoad?: (() => void) | null;
  project: { name: string } | null | undefined;
  usage: ChatUsage | null;
  usageRequest?: AcpUsageRequest | null;
  goal: AcpGoalState;
  onSubmit: (
    event: FormEvent<HTMLFormElement>,
    stagedRevision?: string | null,
  ) => void;
  onTextChange: (text: string) => void;
  onRemoveContext: (context: AttachedContext) => void;
  imageInputAvailable?: boolean;
  imageInputDisabledReason?: string | null;
  onAddImages?: (files: File[]) => void;
  onCancelTurn: (() => void) | null;
  onDeleteQueuedPrompt?: (id: string) => void;
  onRetryQueuedPrompt?: (id: string) => void;
  onSteerQueuedPrompt?: (id: string) => void;
}
