import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MermaidFigure } from "./MermaidFigure";
import { flowchartSource as resetFlowchart } from "../../../../tests/ui/fixtures/cp22Answers";

afterEach(cleanup);

describe("bounded Mermaid flowchart rendering", () => {
  it("reserves unclipped canvas space for an admitted 64-character wide edge label", () => {
    render(<MermaidFigure source={`graph TD\nA -->|${"W".repeat(64)}| B`} />);
    const svg = screen.getByRole("img");
    const [left, , width] = svg.getAttribute("viewBox")!.split(" ").map(Number);
    expect(width).toBeGreaterThan(900);
    expect(left).toBeLessThan(-300);
    expect(left + width).toBeGreaterThan(590);
    expect(svg).toHaveAttribute("width", String(width));
  });
  it.each(["", "unsupported diagram"])(
    "keeps rejected source %j available for exact copying",
    async (source) => {
      const writeText = vi.fn().mockResolvedValue(undefined);
      Object.defineProperty(navigator, "clipboard", {
        configurable: true,
        value: { writeText },
      });
      render(<MermaidFigure source={source} />);
      expect(screen.queryByRole("img")).toBeNull();
      fireEvent.click(
        screen.getByRole("button", { name: "Copy diagram source" }),
      );
      expect(await screen.findByRole("status")).toHaveTextContent("Copied");
      expect(writeText).toHaveBeenCalledWith(source);
    },
  );
  it("reserves body-text room for a long class member and keeps relationships below the member rows", () => {
    const { container } = render(
      <MermaidFigure
        source={`classDiagram\nExample : +${"W".repeat(63)}\nExample : +second\nExample --> Other`}
      />,
    );
    expect(
      Number(screen.getByRole("img").getAttribute("width")),
    ).toBeGreaterThan(1800);
    expect(
      Number(container.querySelector("rect")!.getAttribute("width")),
    ).toBeGreaterThan(900);
    const member = screen.getByText(`+${"W".repeat(63)}`);
    expect(Number(member.getAttribute("x"))).toBeLessThan(30);
    expect(
      Number(container.querySelector("line")!.getAttribute("y1")),
    ).toBeGreaterThan(84);
  });
  it("keeps a wide flowchart at intrinsic size rather than shrinking its labels", () => {
    render(
      <MermaidFigure
        source={"flowchart LR\nBinding --> Pump\nPump --> SQLite"}
      />,
    );
    expect(
      screen.getByRole("img", { name: "Mermaid diagram" }),
    ).toHaveAttribute("width", "920");
    expect(screen.getByRole("img")).toHaveAttribute("height", "100");
    expect(screen.getByRole("img").closest("figure")).toHaveAttribute(
      "tabindex",
      "0",
    );
  });
  it("reports rejected diagram copying truthfully", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    render(<MermaidFigure source={resetFlowchart} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Copy diagram source" }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent("Copy failed");
  });
  it.each([
    ["sequenceDiagram", "Alice->>Bob: message"],
    ["stateDiagram-v2", "Ready --> Done: transition"],
  ])("keeps all admitted rows in the %s coordinate space", (header, edge) => {
    const { container } = render(
      <MermaidFigure source={[header, ...Array(8).fill(edge)].join("\n")} />,
    );
    const height = Number(
      screen.getByRole("img").getAttribute("viewBox")!.split(" ")[3],
    );
    const rows = [...container.querySelectorAll("line[y1]")].map((line) =>
      Number(line.getAttribute("y1")),
    );
    expect(Math.max(...rows)).toBeLessThan(height);
  });
  it.each([
    [
      "sequence",
      "sequenceDiagram\nAlice->>Bob: Hello\nBob-->>Alice: Hi",
      "Hello",
    ],
    ["class", "classDiagram\nAnimal <|-- Duck\nAnimal : +int age", "extends"],
    [
      "state",
      "stateDiagram-v2\n[*] --> Ready\nReady --> Done: finish",
      "finish",
    ],
  ])("preserves the %s subset and labels", (kind, source, label) => {
    render(<MermaidFigure source={source} />);
    expect(
      screen.getByRole("img", { name: `Mermaid ${kind} diagram` }),
    ).toBeVisible();
    expect(screen.getByText(label)).toBeVisible();
  });
  it("wraps six-node labels, points arrows at boundaries and routes the return branch", () => {
    const { container } = render(<MermaidFigure source={resetFlowchart} />);
    const diagram = screen.getByRole("img", { name: "Mermaid diagram" });
    expect(diagram).toHaveAttribute("height", "620");
    expect(container.querySelectorAll("path[marker-end]")).toHaveLength(6);
    expect(container.querySelector("marker")).not.toBeNull();
    expect(container.querySelectorAll("tspan").length).toBeGreaterThan(6);
    expect(screen.getByText("Yes")).toBeVisible();
    expect(screen.getByText("No")).toBeVisible();
    const paths = container.querySelectorAll("path[marker-end]");
    expect(paths[5].getAttribute("d")?.match(/L/g)?.length).toBeGreaterThan(1);
    const positions = [...container.querySelectorAll("g[transform]")].map(
      (node) => node.getAttribute("transform")!.match(/\d+/g)!.map(Number),
    );
    const path = paths[5].getAttribute("d")!;
    expect(path).toMatch(
      new RegExp(`^M${positions[4][0] + 240} ${positions[4][1] + 32}L`),
    );
    expect(path).toMatch(
      new RegExp(`L${positions[0][0] + 240} ${positions[0][1] + 32}$`),
    );
    const connectorX = String(positions[0][0] + 120);
    expect(
      path.match(/\d+ \d+/g)!.map((point) => point.split(" ")[0]),
    ).not.toContain(connectorX);
  });

  it("routes a left-to-right return below its nodes inside the diagram", () => {
    const { container } = render(
      <MermaidFigure source={"graph LR\nA --> B\nB --> A"} />,
    );
    const height = Number(
      screen.getByRole("img").getAttribute("viewBox")!.split(" ")[3],
    );
    const [forward, back] = container.querySelectorAll("path[marker-end]");
    expect(forward.getAttribute("d")).toBe("M260 52L320 52");
    expect(back.getAttribute("d")).toBe("M440 84L440 96L140 96L140 84");
    expect(height).toBeGreaterThan(96);
  });

  it("keeps cycles finite and unsupported markup in fallback", () => {
    const { rerender } = render(
      <MermaidFigure source={"graph LR\nA --> B\nB --> A"} />,
    );
    expect(screen.getByRole("img")).toHaveAttribute(
      "viewBox",
      expect.not.stringContaining("NaN"),
    );
    rerender(
      <MermaidFigure source={"graph TD\nA[<script>bad</script>] --> B"} />,
    );
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByText(/could not be drawn/)).toBeVisible();
  });
});
