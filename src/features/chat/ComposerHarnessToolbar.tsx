import type { ComposerMenuActions } from "./ComposerAddActions";
import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { ChatHarnessControls } from "./ChatHarnessControls";
import { composerWorkspaceName } from "./composerChrome";
import type { ReactNode } from "react";
export function ComposerHarnessToolbar({
  props,
  attachSlot,
  imageAction,
  onOpenGoal,
}: {
  props: ChatComposerRegionProps;
  attachSlot: ReactNode;
  imageAction: ComposerMenuActions["imageAction"];
  onOpenGoal: ComposerMenuActions["onOpenGoal"];
}) {
  const {
    dictationScope,
    sessionId,
    accessTarget,
    onCompact,
    availableBinding,
    canPrompt,
    discoveryPhase,
    sessionHarnessId,
    sessionModelId,
    selectedHarnessId,
    selectedModelId,
    project,
    actionBusy,
    activeTurn,
    sessionConfig,
    modePending,
    modeError,
    focused,
    onSetSessionMode,
    onSetSessionConfigOption,
    onSelectHarness,
    onSelectModel,
    composerRef,
    usage,
    usageRequest,
    draftPickers,
  } = props;
  return (
    <ChatHarnessControls
      modelLabelScope={JSON.stringify([dictationScope, sessionId])}
      inheritScope={dictationScope}
      accessTarget={accessTarget}
      draftPickers={draftPickers}
      addActions={{
        imageAction: imageAction,
        onOpenGoal: onOpenGoal,
        onCompact,
      }}
      attachSlot={attachSlot}
      availableBinding={availableBinding}
      canPrompt={canPrompt}
      discoveryPhase={discoveryPhase}
      sessionHarnessId={sessionHarnessId}
      sessionModelId={sessionModelId}
      selectedHarnessId={selectedHarnessId}
      selectedModelId={selectedModelId}
      projectName={composerWorkspaceName(project)}
      turnBusy={actionBusy || activeTurn}
      sessionConfig={sessionConfig}
      modePending={modePending}
      modeError={modeError}
      focused={focused}
      onSetSessionMode={onSetSessionMode}
      onSetSessionConfigOption={onSetSessionConfigOption}
      onSelectHarness={onSelectHarness}
      onSelectModel={onSelectModel}
      onModeRemoved={() => composerRef.current?.focus()}
      contextUsage={usage}
      usageRequest={usageRequest}
    />
  );
}
