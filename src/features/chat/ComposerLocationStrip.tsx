import { Laptop } from "../../ui/leanIcons";
import { ProjectScopeChip } from "./LazyProjectScopeChip";
import "./composer-location.css";

/**
 * Where a new chat will run, on a tab above its composer as in Codex: this
 * computer, where Sage runs every harness, and the project, which can be
 * switched.
 */
export function ComposerLocationStrip({
  projectName,
  disabled,
}: {
  projectName: string;
  disabled: boolean;
}) {
  return (
    <div className="composer-location" role="group" aria-label="Chat location">
      <span className="composer-location__item">
        <Laptop size={14} aria-hidden="true" />
        This computer
      </span>
      <ProjectScopeChip
        projectName={projectName}
        disabled={disabled}
        popoverPlacement="above-start"
      />
    </div>
  );
}
