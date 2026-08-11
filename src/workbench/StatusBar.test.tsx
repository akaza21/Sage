import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { StatusBar } from "./StatusBar";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

describe("StatusBar repository truth", () => {
  it("shows one repository branch without claiming unsupported status", () => {
    useWorkbenchStore.setState({
      repositories: {
        roots: [
          {
            rootId: "root-a",
            state: "repository",
            branch: "main",
            changes: [],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 1,
      },
      recoveryNotice: null,
    });

    render(<StatusBar />);

    expect(screen.getByText("main")).toBeInTheDocument();
    expect(screen.queryByText("0 problems")).toBeNull();
    expect(screen.queryByText("1 session")).toBeNull();
    expect(screen.queryByText("Trusted project")).toBeNull();
  });

  it("summarizes repositories across multiple roots without choosing one branch", () => {
    useWorkbenchStore.setState({
      repositories: {
        roots: [
          {
            rootId: "root-a",
            state: "repository",
            branch: "main",
            changes: [],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
          {
            rootId: "root-b",
            state: "notRepository",
            branch: null,
            changes: [],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 2,
      },
    });

    render(<StatusBar />);

    expect(screen.getByText("1 repository across 2 roots")).toBeInTheDocument();
    expect(screen.queryByText("main")).toBeNull();
  });

  it("announces a recovery notice only when one exists", () => {
    useWorkbenchStore.setState({
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      recoveryNotice: "Recovered the last Project layout.",
    });

    render(<StatusBar />);

    expect(screen.getByRole("status")).toHaveTextContent(
      "Recovered the last Project layout.",
    );
  });
});
