import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { PickerLogo } from "./PickerOption";
import { SageMark } from "./leanIconCore";

afterEach(cleanup);

it.each(["claude", "codex"])(
  "uses the published %s SVG mark as a decorative inline icon",
  (icon) => {
    const { container } = render(
      <button>
        <PickerLogo icon={icon} />
        {icon}
      </button>,
    );
    const svg = container.querySelector("svg")!;
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("focusable", "false");
    expect(svg.querySelector("use")).toHaveAttribute(
      "href",
      `/harness-${icon}.svg#mark`,
    );
  },
);

it("keeps an existing fallback when no official local mark is available", () => {
  const { container } = render(<PickerLogo logo="/harness-generic.svg" />);
  expect(container.querySelector("img")).toHaveAttribute(
    "src",
    "/harness-generic.svg",
  );
  expect(container.querySelector("img")).toHaveAttribute("aria-hidden", "true");
});

it.each(["/custom-logo.svg", "/custom-logo.png"])(
  "keeps custom logo %s usable with a built-in icon hint",
  (logo) => {
    const { container } = render(<PickerLogo logo={logo} icon="codex" />);
    expect(container.querySelector("img")).toHaveAttribute("src", logo);
    expect(container.querySelector("img")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  },
);

it("shares a 40px decorative outline mark between welcome surfaces", () => {
  const { container } = render(<SageMark />);
  expect(container.querySelector("svg")).toHaveAttribute("fill", "none");
  expect(container.querySelector("svg")).toHaveAttribute("width", "40");
  expect(container.querySelector("use")).toHaveAttribute(
    "href",
    "/sage-mark.svg#mark",
  );
});
