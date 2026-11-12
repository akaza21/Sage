import type { ComponentProps, ReactNode } from "react";
import type { ChatUsage } from "./acp/chatUsage";
import type { ComposerContext } from "./ComposerContext";
import type { ComposerAccessTarget } from "./chatComposerRegionProps";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { ComposerMenuActions } from "./ComposerAddActions";
import type { SessionAccessMode } from "../../domain/sessionAccessMode";
export interface ControlledDraftPickers {
  access: SessionAccessMode | null;
  effort: string | null;
  onAccessChange: (value: SessionAccessMode) => void;
  onEffortChange: (value: string) => void;
}
export interface ChatHarnessControlsProps {
  draftPickers?: ControlledDraftPickers;
  contextUsage?: ChatUsage | null;
  usageRequest?: ComponentProps<typeof ComposerContext>["usageRequest"];
  accessTarget?: ComposerAccessTarget;
  addActions?: ComposerMenuActions;
  /** The attach button, beside the options button. */
  attachSlot?: ReactNode;
  availableBinding: boolean;
  canPrompt: boolean;
  discoveryPhase: HarnessDiscoveryPhase;
  modelLabelScope: string;
  /** The Chat whose parent-inherited model and effort may label the composer. */
  inheritScope?: string;
  sessionHarnessId: string | null;
  sessionModelId: string | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  projectName: string | null;
  turnBusy: boolean;
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
  onModeRemoved: () => void;
}
