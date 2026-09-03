import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { resourceKey } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import { ModalDialog } from "../../ui/ModalDialog";
import { useExplorerActionDialog } from "./explorerActionDialogController";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

function installEligibleFiles(source: ResourceRef, other: ResourceRef) {
  const parent = { rootId: source.rootId, relativePath: "src" };
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: source.rootId,
          name: "Root A",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    projectGeneration: 2,
    nativeTransitionEpoch: 3,
    projectTransitioning: false,
    selectedRootId: source.rootId,
    selectedResource: source,
    tabs: [],
    activeEditor: null,
    directoryListings: {
      [resourceKey(parent)]: {
        phase: "ready",
        value: {
          resource: parent,
          truncated: false,
          entries: [
            { name: "old.ts", resource: source, kind: "file", byteLen: 9 },
            { name: "other.ts", resource: other, kind: "file", byteLen: 9 },
          ],
        },
      },
    },
  });
}

function Harness({
  source,
  mutate,
}: {
  source: ResourceRef;
  mutate: (target: ResourceRef) => Promise<boolean>;
}) {
  const returnFocusRef = useRef<HTMLButtonElement>(null);
  const controller = useExplorerActionDialog({
    clearError: vi.fn(),
    inFlight: false,
    returnFocusRef,
  });

  return (
    <>
      <button ref={returnFocusRef} type="button">
        More
      </button>
      <button
        type="button"
        onClick={() => controller.openDialog({ kind: "file", source })}
      >
        Open
      </button>
      {controller.target ? (
        <ModalDialog
          className="new-file-dialog new-file-dialog--editor"
          labelledBy="shared-dialog-title"
          onDismiss={() => controller.closeDialog()}
        >
          <h2 id="shared-dialog-title">Shared dialog</h2>
          <p>{controller.target.source.relativePath}</p>
          <button
            type="button"
            onClick={() => {
              void controller.runWithTarget((target) => mutate(target.source));
            }}
          >
            Confirm
          </button>
        </ModalDialog>
      ) : null}
    </>
  );
}

describe("useExplorerActionDialog", () => {
  it("keeps the opened target immutable and closes when selection eligibility drifts", async () => {
    const source = { rootId: "root-a", relativePath: "src/old.ts" };
    const other = { rootId: "root-a", relativePath: "src/other.ts" };
    const mutate = vi.fn(async () => false);
    installEligibleFiles(source, other);
    const view = render(<Harness source={source} mutate={mutate} />);

    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("src/old.ts");
    expect(dialog).toHaveClass("new-file-dialog--editor");

    view.rerender(<Harness source={other} mutate={mutate} />);
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await vi.waitFor(() => expect(mutate).toHaveBeenCalledWith(source));

    act(() => useWorkbenchStore.setState({ selectedResource: other }));
    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("closes before mutation when the captured target becomes ineligible", async () => {
    const source = { rootId: "root-a", relativePath: "src/old.ts" };
    const other = { rootId: "root-a", relativePath: "src/other.ts" };
    const mutate = vi.fn(async () => true);
    installEligibleFiles(source, other);
    render(<Harness source={source} mutate={mutate} />);

    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();

    act(() => {
      useWorkbenchStore.setState({
        tabs: [
          {
            id: "open-source",
            kind: "file",
            title: "old.ts",
            position: 0,
            pinned: false,
            dirty: false,
            resource: source,
          },
        ],
      });
    });

    await vi.waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(mutate).not.toHaveBeenCalled();
  });
});
