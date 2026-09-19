import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { ChatUsage } from "./acp/chatUsage";
import { ComposerContext } from "./ComposerContext";

afterEach(cleanup);

function usage(totalTokens: number | null, contextWindow?: number): ChatUsage {
  return {
    inputTokens: totalTokens,
    outputTokens: null,
    cachedInputTokens: null,
    totalTokens,
    ...(contextWindow === undefined ? {} : { contextWindow }),
  };
}

describe("ComposerContext", () => {
  it("keeps unknown context capacity as a localized bare token count", () => {
    render(<ComposerContext project={null} usage={usage(6_000)} />);

    expect(screen.getByLabelText("Composer context")).toHaveTextContent(
      `Reported usage · ${(6_000).toLocaleString()} tokens`,
    );
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("renders reported usage against a known runtime context window", () => {
    render(<ComposerContext project={null} usage={usage(6_000, 272_000)} />);

    expect(screen.getByLabelText("Composer context")).toHaveTextContent(
      `Reported usage · ${(6_000).toLocaleString()} / ${(272_000).toLocaleString()} tokens`,
    );
    const meter = screen.getByRole("progressbar", { name: "Reported usage" });
    expect(meter).toHaveAttribute("aria-valuemin", "0");
    expect(meter).toHaveAttribute("aria-valuemax", "272000");
    expect(meter).toHaveAttribute("aria-valuenow", "6000");
    expect(meter).toHaveAttribute(
      "aria-valuetext",
      `${(6_000).toLocaleString()} / ${(272_000).toLocaleString()} tokens used`,
    );
    expect(meter).not.toHaveTextContent("%");
  });

  it("clamps only the visual and numeric progressbar value when usage is over capacity", () => {
    render(<ComposerContext project={null} usage={usage(300_000, 272_000)} />);

    expect(screen.getByLabelText("Composer context")).toHaveTextContent(
      `Reported usage · ${(300_000).toLocaleString()} / ${(272_000).toLocaleString()} tokens`,
    );
    const meter = screen.getByRole("progressbar", { name: "Reported usage" });
    expect(meter).toHaveAttribute("aria-valuenow", "272000");
    expect(meter).toHaveAttribute(
      "aria-valuetext",
      `${(300_000).toLocaleString()} / ${(272_000).toLocaleString()} tokens used`,
    );
    expect(meter).toHaveStyle({ "--composer-context-usage": "100%" });
  });

  it("does not render a meter for invalid in-memory capacity", () => {
    render(<ComposerContext project={null} usage={usage(6_000, 0)} />);

    expect(screen.getByLabelText("Composer context")).toHaveTextContent(
      `Reported usage · ${(6_000).toLocaleString()} tokens`,
    );
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("does not render usage or a meter without a reported total", () => {
    const { rerender } = render(
      <ComposerContext project={null} usage={usage(null, 272_000)} />,
    );

    expect(screen.queryByText(/Reported usage/)).toBeNull();
    expect(screen.queryByRole("progressbar")).toBeNull();

    rerender(<ComposerContext project={null} usage={null} />);
    expect(screen.queryByText(/Reported usage/)).toBeNull();
    expect(screen.queryByRole("progressbar")).toBeNull();
  });
});
