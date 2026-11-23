import { lucideIcon } from "../../ui/leanIconCore";
import { Laptop } from "../../ui/leanIcons";
import {
  SearchablePicker,
  type SearchablePickerOption,
} from "../../ui/SearchablePicker";
import { ProjectScopeChip } from "./LazyProjectScopeChip";
import { ComposerBranchChip } from "./ComposerBranchChip";
import "./composer-location.css";

const Cloud = lucideIcon("cloud", [
  "M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z",
]);

// Execution targets are picker choices; only the local target is available.
const targets: readonly SearchablePickerOption[] = [
  { id: "local", label: "This computer", leadingIcon: Laptop },
  {
    id: "remote",
    label: "Remote",
    leadingIcon: Cloud,
    disabledReason: "Not set up yet",
  },
];

/**
 * Where a new chat will run, on a tab above its composer as in Codex: this
 * computer, where Sage runs every harness, and the project, which can be
 * switched.
 */
export function ComposerLocationStrip({
  projectName,
  disabled,
  activeTurn = false,
}: {
  projectName: string;
  disabled: boolean;
  activeTurn?: boolean;
}) {
  return (
    <div className="composer-location" role="group" aria-label="Chat location">
      <SearchablePicker
        ariaLabel="Work in"
        appearance="plain"
        menuAppearance
        chevron={false}
        searchPlaceholder="Work in"
        triggerIcon={<Laptop size={14} aria-hidden="true" />}
        popoverPlacement="above-start"
        popoverClassName="composer__popover"
        candidates={targets}
        selectedId="local"
        onSelect={() => {}}
      />
      <ProjectScopeChip
        projectName={projectName}
        disabled={disabled}
        popoverPlacement="above-start"
      />
      <ComposerBranchChip disabled={disabled} activeTurn={activeTurn} />
    </div>
  );
}
