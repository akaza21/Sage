import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { TabKind, WorkbenchTab } from "../domain/workbench";
import { PlaceholderSurface } from "./PlaceholderSurface";

afterEach(cleanup);

describe("PlaceholderSurface legacy restoration", () => {
  it.each<TabKind>(["terminal", "search", "problems", "output", "settings"])(
    "labels a restored %s tab as unavailable without promising future mounting",
    (kind) => {
      const tab: WorkbenchTab = {
        id: `legacy:${kind}`,
        kind,
        title: `Restored ${kind}`,
        position: 0,
        pinned: false,
        dirty: false,
      };

      render(<PlaceholderSurface tab={tab} />);

      expect(
        screen.getByRole("heading", { name: tab.title }),
      ).toBeInTheDocument();
      expect(
        screen.getByText(
          "This legacy tab type is unavailable in this beta. Close it and use the current workbench controls.",
        ),
      ).toBeInTheDocument();
      expect(screen.queryByText(/PTY|CodeMirror/)).toBeNull();
    },
  );
});
