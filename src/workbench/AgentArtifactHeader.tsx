import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import type { AgentArtifactResourceView as ResourceView } from "./AgentArtifactResourceView";
import { SquareArrowOutUpRight, ChevronDown } from "../ui/leanIcons";

import {
  lazy,
  Suspense,
  useId,
  useRef,
  useState,
  type Ref,
  type ComponentProps,
} from "react";

import type { AcpChatPage } from "../domain/acpProjection";
import type { WorkbenchTab } from "../domain/workbench";
import { FilePathChips } from "../features/files/FilePathChips";
import {
  useAgentDiffSourceStore,
  openAgentSnapshotSource,
} from "../app/agentDiffSource";
import { openAgentArtifact } from "../app/openAgentArtifact";
import { useWorkbenchStore } from "../app/store";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { artifactTabParts } from "./agentArtifactTabPresentation";
const loadResourceView = () =>
  import("./AgentArtifactResourceView").then((module) => ({
    default: module.AgentArtifactResourceView,
  }));

export function AgentArtifactResourceView(
  props: Omit<ComponentProps<typeof ResourceView>, "Header">,
) {
  return (
    <LazyModuleBoundary
      load={loadResourceView}
      label="Resource"
      render={(Surface) => <Surface {...props} Header={AgentArtifactHeader} />}
    />
  );
}

const AgentArtifactOpenMenu = lazy(() =>
  import("./AgentArtifactOpenMenu").then((module) => ({
    default: module.AgentArtifactOpenMenu,
  })),
);
const AgentArtifactDetails = lazy(() =>
  import("./AgentArtifactDetails").then((module) => ({
    default: module.AgentArtifactDetails,
  })),
);

export function AgentArtifactHeader({
  tab,
  admitted,
  page,
  position,
  total,
  onStep,
  onOpenInEditor,
  fileActionsRef,
  fileDetailsRef,
}: {
  tab: WorkbenchTab;
  admitted: boolean;
  page: AcpChatPage | null;
  position: number;
  total: number;
  onStep: (direction: -1 | 1) => void;
  onOpenInEditor: () => void;
  fileActionsRef?: Ref<HTMLDivElement>;
  fileDetailsRef?: Ref<HTMLDivElement>;
}) {
  const [openChoices, setOpenChoices] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [error, setError] = useState("");
  const savedSource = useAgentDiffSourceStore((state) =>
    state.sourceReturns.find(
      (entry) =>
        entry.tabId === tab.id &&
        entry.owner.projectId === page?.projectId &&
        entry.owner.chatId === page.chatId &&
        entry.owner.sessionId === page.currentSessionId,
    ),
  );
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const openChoicesId = useId();
  const openChoicesTrigger = useRef<HTMLButtonElement>(null);
  const [path] = artifactTabParts(tab);
  const stats = artifactChipStatsForTab(tab, page);
  async function navigateSaved(source: boolean) {
    setError("");
    try {
      const ref = source ? tab.diffRef : savedSource?.ref;
      const opened =
        ref &&
        (source
          ? await openAgentSnapshotSource(ref)
          : savedSource?.generation === generation &&
            (await openAgentArtifact({
              owner: ref,
              resource: ref,
              target: "diff",
              select: true,
              diffRef: ref,
            })));
      if (!opened)
        setError("This saved change is no longer available in this Chat.");
    } catch {
      setError("Could not open this resource. Try again.");
    }
  }

  async function copyPath() {
    try {
      await navigator.clipboard.writeText(path);
    } catch {
      setError("Could not copy path. Try again.");
    }
  }

  return (
    <header className="agent-artifact-header">
      <div className="agent-artifact-header__title-row">
        <div className="agent-artifact-header__identity" title={path}>
          {savedSource ? (
            <span className="agent-artifact-header__live">Live</span>
          ) : null}
          <FilePathChips path={path} />
        </div>
        {tab.diffRef ? (
          <button
            className="button button--compact"
            type="button"
            onClick={() => void navigateSaved(true)}
          >
            View live source
          </button>
        ) : null}
        {savedSource ? (
          <button
            className="button button--compact"
            type="button"
            onClick={() => void navigateSaved(false)}
          >
            Changes
          </button>
        ) : null}
        {tab.kind === "file" ? (
          <div
            ref={fileActionsRef}
            className="agent-artifact-header__file-actions"
          />
        ) : null}
        <details
          className="agent-artifact-header__details"
          onToggle={(event) => setDetailsOpen(event.currentTarget.open)}
        >
          <summary
            aria-label="Resource details"
            title="View only · Resource details"
          >
            View only
          </summary>
          {detailsOpen ? (
            <Suspense fallback={null}>
              <AgentArtifactDetails
                fileDetailsRef={fileDetailsRef}
                admitted={admitted}
                path={path}
                position={position}
                total={total}
                onStep={onStep}
                stats={stats}
              />
            </Suspense>
          ) : null}
        </details>
        {tab.kind === "file" ? (
          <>
            <button
              ref={openChoicesTrigger}
              className="button button--compact agent-artifact-header__open"
              type="button"
              aria-label="Open choices"
              aria-controls={openChoicesId}
              aria-expanded={openChoices}
              aria-haspopup="menu"
              onClick={() => setOpenChoices((current) => !current)}
            >
              Open <ChevronDown size={12} aria-hidden="true" />
            </button>
            {openChoices ? (
              <Suspense fallback={null}>
                <AgentArtifactOpenMenu
                  id={openChoicesId}
                  trigger={openChoicesTrigger}
                  dismiss={() => setOpenChoices(false)}
                  onOpenInEditor={onOpenInEditor}
                  copyPath={copyPath}
                />
              </Suspense>
            ) : null}
          </>
        ) : (
          <button
            className="button button--compact agent-artifact-header__editor"
            type="button"
            onClick={onOpenInEditor}
          >
            <SquareArrowOutUpRight size={16} aria-hidden="true" />
            <span className="agent-artifact-header__editor-label">
              Open in Editor
            </span>
          </button>
        )}
      </div>
      {error ? <p role="alert">{error}</p> : null}
    </header>
  );
}
