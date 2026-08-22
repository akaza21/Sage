import { useWorkbenchStore } from "../app/store";
import {
  WORKBENCH_WINDOW_MODES,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

const modeLabels: Record<WorkbenchWindowMode, string> = {
  agent: "Agent",
  editor: "Editor",
};

export function TitleBar({
  mode,
  onModeChange,
}: {
  mode: WorkbenchWindowMode;
  onModeChange: (mode: WorkbenchWindowMode) => void;
}) {
  const project = useWorkbenchStore((state) => state.project);

  return (
    <header className="title-bar">
      <div className="title-bar__start">
        <div className="title-bar__brand">
          <span className="title-bar__logo">S</span>
          <strong>Sage</strong>
        </div>
        <div
          className="title-bar__modes"
          role="radiogroup"
          aria-label="Window mode"
        >
          {WORKBENCH_WINDOW_MODES.map((value) => (
            <label
              key={value}
              className={`title-bar__mode${
                mode === value ? " title-bar__mode--active" : ""
              }`}
            >
              <input
                checked={mode === value}
                name="workbench-window-mode"
                onChange={() => onModeChange(value)}
                type="radio"
                value={value}
              />
              {modeLabels[value]}
            </label>
          ))}
        </div>
      </div>
      <div className="title-bar__project">{project?.name ?? "No project"}</div>
      <div className="title-bar__spacer" />
    </header>
  );
}
