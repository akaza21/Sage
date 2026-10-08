import { renderToStaticMarkup } from "react-dom/server";
import * as Lucide from "lucide-react";
import { expect, it } from "vitest";
import * as Lean from "./leanIcons";

it("preserves every moved Lucide SVG and its accessibility attributes", () => {
  expect(Object.keys(Lean)).toHaveLength(97);
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
