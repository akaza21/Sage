import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AgentArtifactHeader } from "./AgentArtifactHeader";

const tab = {
  id: "file:app",
  kind: "file" as const,
  title: "app.ts",
  position: 1,
  pinned: false,
  dirty: false,
  resource: { rootId: "root-a", relativePath: "src/app.ts" },
};

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("AgentArtifactHeader", () => {
  it("resets copy feedback after two seconds", async () => {
    vi.useFakeTimers();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(
      <AgentArtifactHeader
        tab={tab}
        admitted
        page={null}
        position={0}
        total={1}
        onStep={vi.fn()}
        onOpenInEditor={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy path" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByRole("button", { name: "Path copied" })).toBeVisible();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_900);
    });
    fireEvent.click(screen.getByRole("button", { name: "Path copied" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });
    expect(screen.getByRole("button", { name: "Path copied" })).toBeVisible();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_900);
    });
    expect(screen.getByRole("button", { name: "Copy path" })).toBeVisible();
  });
});
