import {
  Maximize2,
  PanelRightClose,
  Plus,
  ChevronLeft,
  Copy,
  FileDiff,
  SquareArrowOutUpRight,
  Check,
  ChevronDown,
  ChevronRight,
} from "../ui/leanIcons";

import { useEffect, useId, useRef, useState, type RefObject } from "react";

import type { AcpChatPage } from "../domain/acpProjection";
import type { WorkbenchTab } from "../domain/workbench";
import { FileKindIcon } from "../features/files/FileKindIcon";
import { MenuPopover } from "../ui/MenuPopover";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { artifactTabParts } from "./agentArtifactTabPresentation";

export function AgentArtifactHeader({
  tab,
  admitted,
  page,
  position,
  total,
  onStep,
  onOpenInEditor,
}: {
  tab: WorkbenchTab;
  admitted: boolean;
  page: AcpChatPage | null;
  position: number;
  total: number;
  onStep: (direction: -1 | 1) => void;
  onOpenInEditor: () => void;
}) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">(
    "idle",
  );
  const [copyFeedbackRevision, setCopyFeedbackRevision] = useState(0);
  const [openChoices, setOpenChoices] = useState(false);
  const openChoicesId = useId();
  const openChoicesTrigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (copyState === "idle") return;
    const timeout = window.setTimeout(() => setCopyState("idle"), 2_000);
    return () => window.clearTimeout(timeout);
  }, [copyState, copyFeedbackRevision]);
  const [path, basename] = artifactTabParts(tab);
  const stats = artifactChipStatsForTab(tab, page);
  const canStep = position >= 0 && total > 1;
  const kindLabel = tab.kind === "diff" ? "Diff" : "File";

  async function copyPath() {
    try {
      await navigator.clipboard.writeText(path);
      setCopyState("copied");
      setCopyFeedbackRevision((revision) => revision + 1);
    } catch {
      setCopyState("error");
      setCopyFeedbackRevision((revision) => revision + 1);
    }
  }

  return (
    <header className="agent-artifact-header">
      {admitted ? (
        <span className="agent-artifact-header__provenance">
          Opened by Sage
        </span>
      ) : null}
      <div className="agent-artifact-header__title-row">
        <div className="agent-artifact-header__identity">
          {tab.kind === "file" ? (
            <FileKindIcon path={path} size={16} />
          ) : (
            <FileDiff size={16} strokeWidth={1.8} aria-hidden="true" />
          )}
          <span className="agent-artifact-header__kind">{kindLabel}</span>
          <strong title={path}>{basename}</strong>
        </div>
        <span className="agent-artifact-header__view-only">VIEW ONLY</span>
        <div className="agent-artifact-header__stepper">
          <button
            type="button"
            aria-label="Previous artifact"
            title="Previous artifact"
            disabled={!canStep || position === 0}
            onClick={() => onStep(-1)}
          >
            <ChevronLeft size={16} aria-hidden="true" />
          </button>
          <span aria-live="polite" aria-atomic="true">
            {position >= 0
              ? `${position + 1} of ${total}`
              : "No artifact selected"}
          </span>
          <button
            type="button"
            aria-label="Next artifact"
            title="Next artifact"
            disabled={!canStep || position === total - 1}
            onClick={() => onStep(1)}
          >
            <ChevronRight size={16} aria-hidden="true" />
          </button>
        </div>
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
            <MenuPopover
              id={openChoicesId}
              open={openChoices}
              ariaLabel="Open file"
              className="menu-popover--editor"
              triggerRef={openChoicesTrigger}
              onDismiss={() => setOpenChoices(false)}
            >
              <button
                className="menu-item"
                type="button"
                role="menuitem"
                onClick={() => {
                  setOpenChoices(false);
                  onOpenInEditor();
                  openChoicesTrigger.current?.focus();
                }}
              >
                <SquareArrowOutUpRight size={16} aria-hidden="true" />
                Open in Editor
              </button>
              <button
                className="menu-item"
                type="button"
                role="menuitem"
                onClick={() => {
                  setOpenChoices(false);
                  void copyPath();
                  openChoicesTrigger.current?.focus();
                }}
              >
                <Copy size={16} aria-hidden="true" />
                Copy path
              </button>
            </MenuPopover>
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
      <div className="agent-artifact-header__path-row">
        <code title={path}>{path}</code>
        {stats?.additionsLabel ? (
          <span className="agent-artifact-header__additions">
            {stats.additionsLabel}
          </span>
        ) : null}
        {stats?.deletionsLabel ? (
          <span className="agent-artifact-header__deletions">
            {stats.deletionsLabel}
          </span>
        ) : null}
        <button
          className="agent-artifact-header__copy"
          type="button"
          aria-label={
            copyState === "copied"
              ? "Path copied"
              : copyState === "error"
                ? "Could not copy path"
                : "Copy path"
          }
          title={copyState === "error" ? "Could not copy path" : "Copy path"}
          onClick={() => void copyPath()}
        >
          {copyState === "copied" ? (
            <Check size={16} aria-hidden="true" />
          ) : (
            <Copy size={16} aria-hidden="true" />
          )}
          <span>{copyState === "copied" ? "Copied" : "Copy path"}</span>
        </button>
      </div>
    </header>
  );
}

// While the resource pane covers the Agent body, the Chat and rail behind it
// leave the tab order and the accessibility tree, and come back on restore.
function useInertSiblings(
  active: boolean,
  anchor: RefObject<HTMLElement | null>,
) {
  useEffect(() => {
    const pane = anchor.current?.closest<HTMLElement>(
      ".agent-artifact-surface",
    );
    if (!active || !pane?.parentElement) return;
    const siblings = Array.from(pane.parentElement.children).filter(
      (node): node is HTMLElement =>
        node instanceof HTMLElement &&
        node !== pane &&
        !node.hasAttribute("inert"),
    );
    for (const node of siblings) node.setAttribute("inert", "");
    return () => {
      for (const node of siblings) node.removeAttribute("inert");
    };
  }, [active, anchor]);
}

export function AgentArtifactPaneControls({
  maximized,
  onMaximizedChange,
  disabled,
  onNewTab,
  onCollapse,
}: {
  maximized: boolean;
  onMaximizedChange: (maximized: boolean) => void;
  disabled: boolean;
  onNewTab: () => void;
  onCollapse: () => void;
}) {
  const expand = useRef<HTMLButtonElement>(null);
  useInertSiblings(maximized, expand);
  return (
    <>
      <button
        className="agent-artifact-pane__collapse agent-artifact-pane__new-tab"
        type="button"
        aria-label="New browser tab"
        title="New browser tab"
        disabled={disabled}
        onClick={onNewTab}
      >
        <Plus size={16} aria-hidden="true" />
      </button>
      <button
        ref={expand}
        className="agent-artifact-pane__collapse"
        type="button"
        aria-label={
          maximized ? "Restore resource pane" : "Expand resource pane"
        }
        title={maximized ? "Restore resource pane" : "Expand resource pane"}
        aria-pressed={maximized}
        onClick={() => onMaximizedChange(!maximized)}
      >
        <Maximize2 size={14} aria-hidden="true" />
      </button>
      <button
        className="agent-artifact-pane__collapse"
        type="button"
        aria-label="Collapse files, changes, and preview"
        title="Collapse resources"
        onClick={onCollapse}
      >
        <PanelRightClose size={16} aria-hidden="true" />
      </button>
    </>
  );
}
