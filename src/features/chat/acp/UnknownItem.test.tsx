import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { UnknownItem } from "./UnknownItem";

afterEach(cleanup);

describe("UnknownItem", () => {
  it("renders only the normalized bounded fallback", () => {
    const item = {
      kind: "unsupported",
      payload: {
        category: "extension.future",
        summary: "A future item was retained safely.",
        truncated: true,
      },
    } as Extract<AcpItem, { kind: "unsupported" }>;
    render(<UnknownItem item={item} />);
    expect(screen.getByLabelText("Unsupported agent item")).toHaveTextContent(
      "A future item was retained safely.",
    );
    expect(screen.getByText("Summary truncated")).toBeInTheDocument();
  });
});
