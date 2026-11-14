import { Laptop } from "../../ui/leanIcons";
import { ProjectScopeChip } from "./ProjectScopeChip";
import "./composer-location.css";

/**
 * Where a new chat will run, above its input as in Codex: this computer,
 * where Sage runs every harness, and the project, which can be switched.
 */
export function ComposerLocationStrip({
  projectName,
  disabled,
}: {
  projectName: string;
  disabled: boolean;
}) {
  return (
    <div className="composer__location" role="group" aria-label="Chat location">
      <span className="composer__location-item">
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
