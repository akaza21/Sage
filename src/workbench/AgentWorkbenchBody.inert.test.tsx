import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState, type ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { AgentWorkbenchBody } from "./AgentWorkbenchBody";
import { agentWorkbenchLayout } from "./agentWorkbenchLayout";
vi.mock("./LeftSidebar", () => ({
  LeftSidebar: ({ inert }: { inert: boolean }) => (
    <nav data-testid="rail" inert={inert} />
  ),
}));
vi.mock("./AgentChatPanes", () => ({
  AgentChatPanes: () => <p>Chat content</p>,
}));
vi.mock("./AgentToolsProvider", () => ({
  AgentToolsProvider: ({ children }: { children: ReactNode }) => children,
}));
vi.mock("./AgentArtifactPane", () => ({
  AgentArtifactPane: ({
    onMaximizedChange,
    onExpandedChange,
  }: {
    onMaximizedChange: (next: boolean) => void;
    onExpandedChange: (next: boolean) => void;
  }) => (
    <aside>
      <button onClick={() => onMaximizedChange(true)}>Maximize</button>
      <button onClick={() => onExpandedChange(false)}>External collapse</button>
    </aside>
  ),
}));
afterEach(cleanup);
it("releases the Chat and rail after an external collapse of maximized resources", () => {
  function WorkbenchBody() {
    const [expanded, setExpanded] = useState(true);
    return (
      <AgentWorkbenchBody
        layout={agentWorkbenchLayout({
          viewportWidth: 1600,
          artifactExpanded: expanded,
        })}
        style={undefined}
        drawerOpen={false}
        onExpandNavigation={() => undefined}
        artifactExpanded={expanded}
        onArtifactExpandedChange={setExpanded}
        onAgentLeftRailWidthChange={() => undefined}
        onAgentArtifactWidthChange={() => undefined}
        onOpenArtifactInEditor={() => undefined}
      />
    );
  }
  const view = render(<WorkbenchBody />);
  fireEvent.click(screen.getByRole("button", { name: "Maximize" }));
  expect(screen.getByTestId("rail")).toHaveAttribute("inert");
  fireEvent.click(screen.getByRole("button", { name: "External collapse" }));
  expect(screen.getByTestId("rail")).not.toHaveAttribute("inert");
  expect(view.container.querySelector(".center-region")).not.toHaveAttribute(
    "inert",
  );
});
