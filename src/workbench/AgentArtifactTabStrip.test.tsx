import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import { AgentArtifactTabStrip } from "./AgentArtifactTabStrip";
import type { WorkbenchTab } from "../domain/workbench";

afterEach(cleanup);

function Harness({ deferredClose = false }: { deferredClose?: boolean }) {
  const [pendingClose, setPendingClose] = useState<string | null>(null);
  const [tabs, setTabs] = useState<WorkbenchTab[]>(
    ["a.ts", "b.ts", "c.ts"].map((title) => ({
      id: title,
      kind: "file",
      title,
      resource: { rootId: "root", relativePath: title },
      dirty: false,
      pinned: false,
      position: 0,
    })),
  );
  const [active, setActive] = useState("b.ts");
  return (
    <>
      <AgentArtifactTabStrip
        artifacts={tabs}
        selectedTarget={{ kind: "tab", tabId: active }}
        focusedOwner={null}
        focusedPage={null}
        previewSelected={false}
        previewOpen={false}
        subagentOpen={false}
        subagentSelected={false}
        onSelectSubagent={() => undefined}
        record={null}
        availableTargets={[]}
        activateTab={setActive}
        closeTab={(id) =>
          deferredClose
            ? setPendingClose(id)
            : setTabs((current) => current.filter((tab) => tab.id !== id))
        }
        panelId="test-panel"
      />
      <div id="test-panel" role="tabpanel">
        {active}
      </div>
      {pendingClose ? (
        <button
          onClick={() => {
            setTabs((current) =>
              current.filter((tab) => tab.id !== pendingClose),
            );
            setPendingClose(null);
          }}
        >
          Confirm close
        </button>
      ) : null}
    </>
  );
}

it("roves and selects with arrows and Home/End, associates the panel, and focuses the surviving close neighbor", async () => {
  render(<Harness />);
  const middle = screen.getByRole("tab", { name: "File b.ts" });
  expect(middle).toHaveAttribute("tabindex", "0");
  expect(screen.getByRole("tab", { name: "File a.ts" })).toHaveAttribute(
    "tabindex",
    "-1",
  );
  fireEvent.keyDown(middle, { key: "End" });
  const last = screen.getByRole("tab", { name: "File c.ts" });
  expect(last).toHaveFocus();
  await waitFor(() => expect(last).toHaveAttribute("aria-selected", "true"));
  expect(screen.getByRole("tabpanel")).toHaveAttribute(
    "aria-labelledby",
    last.id,
  );
  fireEvent.keyDown(last, { key: "Home" });
  const first = screen.getByRole("tab", { name: "File a.ts" });
  expect(first).toHaveFocus();
  fireEvent.keyDown(first, { key: "ArrowRight" });
  expect(middle).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "Close b.ts" }));
  await waitFor(() => expect(last).toHaveFocus());
  expect(last).toHaveAttribute("aria-selected", "true");
});

it("keeps selection while close is pending and focuses a neighbor only after actual removal", async () => {
  render(<Harness deferredClose />);
  const closing = screen.getByRole("button", { name: "Close b.ts" });
  closing.focus();
  fireEvent.click(closing);
  expect(screen.getByRole("tab", { name: "File b.ts" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  expect(closing).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "Confirm close" }));
  await waitFor(() =>
    expect(screen.getByRole("tab", { name: "File c.ts" })).toHaveFocus(),
  );
  expect(screen.getByRole("tab", { name: "File c.ts" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});
