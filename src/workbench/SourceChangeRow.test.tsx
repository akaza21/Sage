import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import type { ComponentProps } from "react";
import type * as ConflictPresentation from "../features/git/conflictPresentation";
import type { FileChange, ResourceRef } from "../domain/workbench";
import { SourceChangeGroups } from "./SourceChangeGroups";

function SourceChangeRow({
  change,
  isSelected,
  ...props
}: Omit<
  ComponentProps<typeof SourceChangeGroups>,
  | "changes"
  | "selected"
  | "rebaseInProgress"
  | "cherryPickInProgress"
  | "revertInProgress"
> & { change: FileChange; isSelected: boolean }) {
  return (
    <SourceChangeGroups
      {...props}
      changes={[change]}
      selected={isSelected ? change.resource : null}
      rebaseInProgress={false}
      cherryPickInProgress={false}
      revertInProgress={false}
    />
  );
}

const { sourceChangeStatusChip } = vi.hoisted(() => ({
  sourceChangeStatusChip: vi.fn(() => "M"),
}));

vi.mock("../features/git/conflictPresentation", async (importOriginal) => ({
  ...(await importOriginal<typeof ConflictPresentation>()),
  sourceChangeStatusChip,
}));

afterEach(cleanup);

const resource = {
  rootId: "root-a",
  relativePath: "src/main.ts",
} as ResourceRef;
const change = {
  resource,
  worktreeStatus: "modified",
  indexStatus: "unchanged",
  additions: 1,
  deletions: 0,
} as FileChange;

it("keeps row actions fresh across a memoized selected-resource update", () => {
  const openDiff = vi.fn(async () => undefined);
  const mutate = vi.fn(async () => true);
  const props = {
    change,
    repository: undefined,
    rootName: "A",
    controlsDisabled: false,
    mutateRepositoryFile: mutate,
    openDiff,
  };
  const { rerender } = render(
    <SourceChangeRow {...props} isSelected={false} />,
  );
  rerender(<SourceChangeRow {...props} isSelected />);
  const open = screen.getByRole("button", {
    name: "Open diff for src/main.ts",
  });
  expect(open).toHaveAttribute("aria-current", "true");
  fireEvent.click(
    screen.getByRole("button", { name: "Stage src/main.ts in A" }),
  );
  expect(mutate).toHaveBeenCalledWith(resource, "stage");
});

it("updates only selected Source Change rows without staling their actions", () => {
  const secondResource = {
    rootId: "root-a",
    relativePath: "src/second.ts",
  } as ResourceRef;
  const unchangedResource = {
    rootId: "root-a",
    relativePath: "src/unchanged.ts",
  } as ResourceRef;
  const changes: FileChange[] = [
    change,
    { ...change, resource: secondResource },
    { ...change, resource: unchangedResource },
  ];
  const openDiff = vi.fn(async () => undefined);
  const mutate = vi.fn(async () => true);
  const props = {
    repository: undefined,
    rootName: "A",
    controlsDisabled: false,
    mutateRepositoryFile: mutate,
    openDiff,
  };
  const ChangeRows = ({ selected }: { selected: ResourceRef | null }) => (
    <SourceChangeGroups
      {...props}
      changes={changes}
      selected={selected}
      rebaseInProgress={false}
      cherryPickInProgress={false}
      revertInProgress={false}
    />
  );
  const { rerender } = render(<ChangeRows selected={resource} />);
  sourceChangeStatusChip.mockClear();

  rerender(<ChangeRows selected={secondResource} />);

  expect(sourceChangeStatusChip).toHaveBeenCalledTimes(2);
  fireEvent.click(
    screen.getByRole("button", { name: "Open diff for src/second.ts" }),
  );
  expect(openDiff).toHaveBeenCalledWith(secondResource, undefined);
});
