import { renderToStaticMarkup } from "react-dom/server";
import { createRef } from "react";
import { render } from "@testing-library/react";
import * as Lucide from "lucide-react";
import { expect, it } from "vitest";
import * as Lean from "./leanIcons";

it.each(["MoreHorizontal", "TriangleAlert", "XCircle"] as const)(
  "preserves SVG refs and children for %s",
  (name) => {
    const Icon = Lean[name];
    const ref = createRef<SVGSVGElement>();
    const { getByLabelText, unmount } = render(
      <Icon ref={ref} aria-label={name}>
        <title>{name}</title>
      </Icon>,
    );
    expect(ref.current).toBe(getByLabelText(name));
    expect(ref.current?.querySelector("title")).toHaveTextContent(name);
    unmount();
    expect(ref.current).toBeNull();
  },
);

it("preserves every moved Lucide SVG and its accessibility attributes", () => {
  expect(Object.keys(Lean)).toHaveLength(105);
  for (const [name, Icon] of Object.entries(Lean)) {
    const Original = Lucide[name as keyof typeof Lucide] as typeof Icon;
    expect(renderToStaticMarkup(<Icon />), name).toBe(
      renderToStaticMarkup(<Original />),
    );
    expect(
      renderToStaticMarkup(
        <Icon
          size={14}
          strokeWidth={1.8}
          className="custom"
          aria-label="Icon"
        />,
      ),
      name,
    ).toBe(
      renderToStaticMarkup(
        <Original
          size={14}
          strokeWidth={1.8}
          className="custom"
          aria-label="Icon"
        />,
      ),
    );
    expect(
      renderToStaticMarkup(
        <Icon size={16} absoluteStrokeWidth color="red" role="img" />,
      ),
      name,
    ).toBe(
      renderToStaticMarkup(
        <Original size={16} absoluteStrokeWidth color="red" role="img" />,
      ),
    );
  }
});
