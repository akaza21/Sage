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

/** Whose access the composer offers: the session's binding, the chat while
 * it has no session at all, whose access is then drafted, and the session
 * holding the chat's access while none is live. */
export interface ComposerAccessTarget {
  bindingId?: string | null;
  draftChatId?: string;
  heldSessionId?: string;
}

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
  /** The session the next send uses, for whether it takes attached text. */
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
  /** The harness that refused for sign-in; its guidance replaces chatError. */
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
  /** Sends, naming the staged batch revision the message carries. */
  onSubmit: (
    event: FormEvent<HTMLFormElement>,
    stagedRevision?: string | null,
  ) => void;
  onTextChange: (text: string) => void;
  onRemoveContext: (context: AttachedContext) => void;
  imageInputAvailable?: boolean;
  /** Why image input is unavailable, shown when an image is offered anyway. */
  imageInputDisabledReason?: string | null;
  onAddImages?: (files: File[]) => void;
  onCancelTurn: (() => void) | null;
  onDeleteQueuedPrompt?: (id: string) => void;
  onRetryQueuedPrompt?: (id: string) => void;
  onSteerQueuedPrompt?: (id: string) => void;
}
