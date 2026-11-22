import type { ControlledDraftPickers } from "./chatHarnessControlsProps";
import type { ComposerProblem } from "./ComposerStatusNotices";
import type { FormEvent, ReactNode, RefObject } from "react";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type { AcpGoalCapabilities, AcpGoalState } from "../../domain/acpGoal";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { AttachedContext } from "../../domain/attachedContext";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";
import type { ChatUsage } from "./acp/chatUsage";
import type { PasteAttachability } from "./composerPasteText";
import type { EndedPage } from "./endedSession";
import type { AcpUsageRequest } from "../../domain/acpUsage";
import type { AcpChatPage, AcpCommandFailure } from "../../domain/acp";

export interface ComposerAccessTarget {
  bindingId?: string | null;
  draftChatId?: string;
  endedPage?: EndedPage;
}

export interface ChatComposerRegionProps {
  draftPickers?: ControlledDraftPickers;
  /** Kept mounted while an approval holds the slot, so the draft survives. */
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
  workbenchError?: ComposerProblem | null;
  chatError: ComposerProblem | null;
  chatAuth?: {
    page: AcpChatPage | null | undefined;
    errorKind: AcpCommandFailure["kind"] | null;
    boundChatId?: string | null;
  };
  accessTarget?: ComposerAccessTarget;
  availableBinding: boolean;
  canPrompt: boolean;
  discoveryPhase: HarnessDiscoveryPhase;
  sessionHarnessId?: string | null;
  sessionModelId?: string | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  sessionPromptUnavailable?: boolean;
  sessionConfig: AcpSessionConfig | null;
  modePending: boolean;
  modeError?: string | null;
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
  /** The chat's project; none for a side chat, which cannot switch it. */
  project: { name: string; isInternal?: boolean } | null | undefined;
  newChat?: boolean;
  usage?: ChatUsage | null;
  usageRequest?: AcpUsageRequest | null;
  goal?: AcpGoalState;
  /** Native goal operations of the live session; absent while offline. */
  goalCapabilities?: AcpGoalCapabilities;
  onSubmit: (
    event: FormEvent<HTMLFormElement>,
    stagedRevision?: string | null,
  ) => void;
  onTextChange: (text: string) => void;
  onRemoveContext: (context: AttachedContext) => void;
  imageInputAvailable?: boolean;
  imageInputDisabledReason?: string | null;
  onAddImages?: (files: File[]) => void;
  onCancelTurn?: (() => void) | null;
  onDeleteQueuedPrompt?: (id: string) => void;
  onRetryQueuedPrompt?: (id: string) => void;
  onSteerQueuedPrompt?: (id: string) => void;
}
