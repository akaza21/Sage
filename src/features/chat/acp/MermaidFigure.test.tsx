import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MermaidFigure } from "./MermaidFigure";
import { flowchartSource as resetFlowchart } from "../../../../tests/ui/fixtures/cp22Answers";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("bounded Mermaid flowchart rendering", () => {
  it("preserves the natural routing canvas and delegates scaling to CSS", () => {
    render(<MermaidFigure source={"graph TD\nA --> B"} />);
    const svg = screen.getByRole("img");
    expect(svg).toHaveAttribute("width", "320");
    expect(svg).toHaveAttribute("viewBox", "0 0 320 204");
    expect(svg.style.getPropertyValue("--mermaid-width")).toBe("320px");
  });
  it.each([
    ["OK", 64, 240],
    ["Token and password valid?", 128, 300],
  ] as const)(
    "fits the wrapped decision '%s' in a compact diamond with central label anchors",
    (label, maximumHeight, maximumWidth) => {
      const { container } = render(
        <MermaidFigure
          source={`graph TD\nDecision{${label}}\nText[${label}]`}
        />,
      );
      const diamond = container.querySelector("polygon")!;
      const points = diamond
        .getAttribute("points")!
        .split(" ")
        .map((point) => point.split(",").map(Number));
      const width = Math.max(...points.map((point) => point[0]));
      const height = Math.max(...points.map((point) => point[1]));
      const rectangle = container.querySelector("rect")!;
      expect(width).toBeLessThanOrEqual(
        Number(rectangle.getAttribute("width")),
      );
      expect(width).toBeLessThanOrEqual(maximumWidth);
      expect(height).toBeLessThanOrEqual(maximumHeight);
      const text = diamond.parentElement!.querySelector("text")!;
      const lines = [...text.querySelectorAll("tspan")];
      expect(lines.map((line) => line.textContent).join(" ")).toBe(label);
      let baseline = Number(text.getAttribute("y"));
      for (const line of lines) {
        baseline += Number(line.getAttribute("dy"));
        expect(Number(line.getAttribute("x"))).toBe(width / 2);
        expect(baseline).toBeGreaterThan(height / 4);
        expect(baseline).toBeLessThan((height * 3) / 4);
      }
    },
  );
  it.each(
    ["TD", "LR"].flatMap((direction) =>
      [28, 64].flatMap((length) =>
        [
          ["[", "]"],
          ["(", ")"],
          ["{", "}"],
        ].map(([open, close]) => ({ direction, length, open, close })),
      ),
    ),
  )(
    "wraps $length-character $open labels in disjoint bounded $direction nodes",
    ({ direction, length, open, close }) => {
      const label = "W".repeat(length);
      const { container } = render(
        <MermaidFigure
          source={`graph ${direction}\nStart --> Left${open}${label}${close}\nStart --> Right${open}${label}${close}\nRight --> End`}
        />,
      );
      const nodes = [...container.querySelectorAll<SVGGElement>("svg > g")]
        .filter((group) => group.querySelector("rect, polygon"))
        .map((group) => {
          const [left, top] = group
            .getAttribute("transform")!
            .match(/[\d.]+/gu)!
            .map(Number);
          const shape = group.querySelector("rect, polygon")!;
          const points = shape
            .getAttribute("points")
            ?.split(" ")
            .map((point) => point.split(",").map(Number));
          const text = group.querySelector("text")!;
          const lines = [...text.querySelectorAll("tspan")];
          const right =
            left +
            (points
              ? Math.max(...points.map((point) => point[0]))
              : Number(shape.getAttribute("width")));
          const bottom =
            top +
            (points
              ? Math.max(...points.map((point) => point[1]))
              : Number(shape.getAttribute("height")));
          return {
            left,
            top,
            right,
            bottom,
            lines,
          };
        });
      expect(nodes).toHaveLength(4);
      for (const [index, node] of nodes.entries()) {
        for (const other of nodes.slice(index + 1)) {
          expect(
            node.right <= other.left ||
              other.right <= node.left ||
              node.bottom <= other.top ||
              other.bottom <= node.top,
          ).toBe(true);
        }
        expect(node.right - node.left).toBeLessThanOrEqual(320);
        expect(node.bottom).toBeGreaterThan(node.top);
      }
      for (const node of nodes.slice(1, 3)) {
        expect(node.lines.length).toBeGreaterThan(1);
        expect(node.lines.map((line) => line.textContent).join("")).toBe(label);
      }
    },
  );
  it.each(
    [
      { direction: "TD", label: "A much longer label here ok" },
      { direction: "LR", label: "W".repeat(64) },
    ].flatMap((diagram) =>
      [false, true].flatMap((sibling) =>
        ["", "retry"].map((edgeLabel) => ({ ...diagram, sibling, edgeLabel })),
      ),
    ),
  )(
    "routes a $direction narrow-wide-narrow return outside all spanned nodes (sibling=$sibling, label='$edgeLabel')",
    ({ direction, label, sibling, edgeLabel }) => {
      const { container } = render(
        <MermaidFigure
          source={`flowchart ${direction}\nA[Go] --> B[${label}]\nB --> C[Done]\n${sibling ? `A --> D[${label}]\n` : ""}C -->${edgeLabel ? `|${edgeLabel}|` : ""} A`}
        />,
      );
      const nodes = [...container.querySelectorAll("g[transform]")].map(
        (node) => {
          const [left, top] = node
            .getAttribute("transform")!
            .match(/[\d.]+/gu)!
            .map(Number);
          const shape = node.querySelector("rect")!;
          const width = Number(shape.getAttribute("width"));
          const height = Number(shape.getAttribute("height"));
          return { width, height, right: left + width, bottom: top + height };
        },
      );
      expect(nodes).toHaveLength(sibling ? 4 : 3);
      const dimension = direction === "TD" ? "width" : "height";
      expect(nodes[1][dimension]).toBeGreaterThan(nodes[0][dimension]);
      expect(nodes[1][dimension]).toBeGreaterThan(nodes[2][dimension]);
      const points = [...container.querySelectorAll("path[marker-end]")]
        .at(-1)!
        .getAttribute("d")!
        .match(/[ML][\d.]+ [\d.]+/gu)!
        .map((point) => point.slice(1).split(" ").map(Number));
      const axis = direction === "TD" ? 0 : 1;
      expect(points).toHaveLength(4);
      expect(points[1][axis]).toBe(points[2][axis]);
      for (const node of nodes)
        expect(points[1][axis]).toBeGreaterThan(
          direction === "TD" ? node.right : node.bottom,
        );
      const [svgLeft, , svgWidth, svgHeight] = screen
        .getByRole("img")
        .getAttribute("viewBox")!
        .split(" ")
        .map(Number);
      expect(points[1][axis]).toBeLessThan(
        direction === "TD" ? svgLeft + svgWidth : svgHeight,
      );
    },
  );
  it("only makes an overflowing figure focusable and updates on resize and source replacement", () => {
    let resize = () => {};
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: () => void) {
          resize = callback;
        }
        observe() {}
        disconnect() {}
      },
    );
    const scrollWidth = vi
      .spyOn(HTMLElement.prototype, "scrollWidth", "get")
      .mockReturnValue(100);
    const clientWidth = vi
      .spyOn(HTMLElement.prototype, "clientWidth", "get")
      .mockReturnValue(100);
    const { rerender } = render(<MermaidFigure source={"graph TD\nA --> B"} />);
    const figure = screen.getByRole("figure");
    expect(figure).not.toHaveAttribute("tabindex");
    scrollWidth.mockReturnValue(101);
    act(() => resize());
    expect(figure).toHaveAttribute("tabindex", "0");
    clientWidth.mockReturnValue(101);
    act(() => resize());
    expect(figure).not.toHaveAttribute("tabindex");
    clientWidth.mockReturnValue(100);
    rerender(<MermaidFigure source={"graph LR\nA --> B"} />);
    expect(figure).toHaveAttribute("tabindex", "0");
    rerender(<MermaidFigure source="unsupported diagram" />);
    expect(figure).not.toHaveAttribute("tabindex");
  });
  it("distinguishes figures even when their leading labels or entire sources match", () => {
    render(
      <>
        <MermaidFigure source={"graph TD\nA[Go] -->|again| B[Done]"} />
        <MermaidFigure source={"graph TD\nA[Go] -->|stop| B[Done]"} />
        <MermaidFigure source={"graph TD\nA[Go] -->|again| B[Done]"} />
        <MermaidFigure source={"graph TD\nA[Read] --> B[Write]"} />
      </>,
    );
    const figures = screen.getAllByRole("figure");
    const names = figures.map((figure) => figure.getAttribute("aria-label"));
    expect(new Set(names).size).toBe(4);
    for (const figure of figures.slice(0, 3))
      expect(figure).toHaveAccessibleName(/^Diagram: Go, Done/u);
    expect(figures[3]).toHaveAccessibleName(/^Diagram: Read, Write/u);
  });
  it("exposes every flowchart node and edge label in its text alternative", () => {
    render(
      <MermaidFigure
        source={
          "graph TD\nA[Go] -->|again| B[Wait]\nB -->|finish| C[Done]\nC --> A"
        }
      />,
    );
    expect(screen.getByRole("img")).toHaveAccessibleDescription(
      "Nodes: Go; Wait; Done. Connections: Go to Wait: again; Wait to Done: finish; Done to Go.",
    );
  });
  it("makes a vertically overflowing figure focusable", () => {
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(101);
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(100);
    render(<MermaidFigure source={"graph TD\nA --> B"} />);
    expect(screen.getByRole("figure")).toHaveAttribute("tabindex", "0");
  });
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
  it("keeps exact-source copying and its status outside the diagram scroller", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<MermaidFigure source={resetFlowchart} />);
    const figure = screen.getByRole("figure");
    const copy = screen.getByRole("button", { name: "Copy diagram source" });
    expect(copy).toHaveAttribute("title", "Copy diagram source");
    expect(copy.textContent).toBe("");
    expect(copy.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(figure).not.toContainElement(copy);
    expect(figure.parentElement).toContainElement(copy);
    fireEvent.click(copy);
    const status = await screen.findByRole("status");
    expect(status).toHaveTextContent("Copied");
    expect(figure).not.toContainElement(status);
    expect(figure.parentElement).toContainElement(status);
    expect(writeText).toHaveBeenCalledWith(resetFlowchart);
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
      "Nodes: Alice; Bob. Connections: Alice to Bob: Hello; Bob to Alice: Hi.",
    ],
    [
      "class",
      "classDiagram\nAnimal <|-- Duck\nAnimal : +int age",
      "extends",
      "Nodes: Animal, +int age; Duck. Connections: Animal to Duck: extends.",
    ],
    [
      "state",
      "stateDiagram-v2\n[*] --> Ready\nReady --> Done: finish",
      "finish",
      "Nodes: [*]; Ready; Done. Connections: [*] to Ready; Ready to Done: finish.",
    ],
  ])(
    "preserves the %s subset with node and edge text alternatives",
    (kind, source, label, description) => {
      render(<MermaidFigure source={source} />);
      expect(
        screen.getByRole("img", { name: `Mermaid ${kind} diagram` }),
      ).toBeVisible();
      expect(screen.getByText(label)).toBeVisible();
      expect(screen.getByRole("img")).toHaveAccessibleDescription(description);
    },
  );
  it("wraps six-node labels, points arrows at boundaries and routes the return branch", () => {
    const { container } = render(<MermaidFigure source={resetFlowchart} />);
    const diagram = screen.getByRole("img", { name: "Mermaid diagram" });
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
    const decision = container
      .querySelector("polygon")!
      .getAttribute("points")!
      .split(" ")
      .map((point) => point.split(",").map(Number));
    const decisionWidth = Math.max(...decision.map((point) => point[0]));
    const decisionHeight = Math.max(...decision.map((point) => point[1]));
    const firstWidth = Number(
      container.querySelector("rect")!.getAttribute("width"),
    );
    const lastNode = [...container.querySelectorAll("g[transform]")].at(-1)!;
    expect(Number(diagram.getAttribute("height"))).toBeGreaterThan(
      positions.at(-1)![1] +
        Number(lastNode.querySelector("rect")!.getAttribute("height")),
    );
    const path = paths[5].getAttribute("d")!;
    expect(path).toMatch(
      new RegExp(
        `^M${positions[4][0] + decisionWidth} ${positions[4][1] + decisionHeight / 2}L`,
      ),
    );
    expect(path).toMatch(
      new RegExp(
        `L${positions[0][0] + firstWidth} ${positions[0][1] + Number(container.querySelector("rect")!.getAttribute("height")) / 2}$`,
      ),
    );
    const connectorX = String(positions[0][0] + firstWidth / 2);
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
