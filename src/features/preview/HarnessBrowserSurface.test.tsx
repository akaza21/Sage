import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { HarnessBrowserSurface } from "./HarnessBrowserSurface";
import { applyBrowseIntent, clearBrowseSessions } from "./harnessBrowse";

afterEach(() => {
  cleanup();
  clearBrowseSessions();
});

describe("HarnessBrowserSurface", () => {
  it("shows the admitted loopback page for that chat and refuses a missing session", () => {
    applyBrowseIntent({
      chatId: "chat:a",
      url: "http://127.0.0.1:5173/app",
    });
    const { rerender } = render(<HarnessBrowserSurface chatId="chat:a" />);
    const frame = screen.getByTitle("Sage browser");
    expect(frame).toHaveAttribute("src", "http://127.0.0.1:5173/app");
    expect(screen.getByLabelText("Preview address")).toHaveValue(
      "http://127.0.0.1:5173/app",
    );

    rerender(<HarnessBrowserSurface chatId="chat:b" />);
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(screen.getByText(/Loopback HTTP\(S\) only/i)).toBeInTheDocument();
  });

  it("opens an admitted address and refuses public HTTPS", () => {
    render(<HarnessBrowserSurface chatId="chat:a" />);
    const address = screen.getByLabelText("Preview address");
    fireEvent.change(address, {
      target: { value: "https://example.com/docs" },
    });
    fireEvent.submit(address.closest("form")!);
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Only loopback HTTP(S) with a port",
    );

    fireEvent.change(address, {
      target: { value: "http://127.0.0.1:4173/preview" },
    });
    fireEvent.submit(address.closest("form")!);
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:4173/preview",
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("reloads the admitted page without leaving Sage", () => {
    applyBrowseIntent({
      chatId: "chat:a",
      url: "http://127.0.0.1:5173/app",
    });
    render(<HarnessBrowserSurface chatId="chat:a" />);
    const first = screen.getByTitle("Sage browser");
    fireEvent.click(screen.getByRole("button", { name: "Reload preview" }));
    const reloaded = screen.getByTitle("Sage browser");
    expect(reloaded).toHaveAttribute("src", "http://127.0.0.1:5173/app");
    expect(reloaded).not.toBe(first);
  });

  it("keeps the browser chrome controls named in DOM order", () => {
    applyBrowseIntent({
      chatId: "chat:a",
      url: "http://127.0.0.1:5173/app",
    });
    render(<HarnessBrowserSurface chatId="chat:a" />);

    const chrome = screen.getByRole("region", { name: "Sage browser" });
    expect(
      Array.from(chrome.querySelectorAll("button, input")).map((element) =>
        element.getAttribute("aria-label"),
      ),
    ).toEqual([
      "Back",
      "Forward",
      "Preview address",
      "Reload preview",
      "Close browser",
    ]);
  });

  it("walks admitted back and forward without leaving Sage", () => {
    applyBrowseIntent({
      chatId: "chat:a",
      url: "http://127.0.0.1:5173/one",
    });
    applyBrowseIntent({
      chatId: "chat:a",
      url: "http://127.0.0.1:5173/two",
    });
    render(<HarnessBrowserSurface chatId="chat:a" />);
    const back = screen.getByRole("button", { name: "Back" });
    const forward = screen.getByRole("button", { name: "Forward" });
    expect(back).toBeEnabled();
    expect(forward).toBeDisabled();
    fireEvent.click(back);
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/one",
    );
    expect(screen.getByLabelText("Preview address")).toHaveValue(
      "http://127.0.0.1:5173/one",
    );
    expect(forward).toBeEnabled();
    fireEvent.click(forward);
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/two",
    );
  });

  it("clears the chat session without opening an external window", () => {
    applyBrowseIntent({
      chatId: "chat:a",
      url: "http://127.0.0.1:4173/",
    });
    render(<HarnessBrowserSurface chatId="chat:a" />);
    fireEvent.click(screen.getByRole("button", { name: "Close browser" }));
    expect(screen.queryByTitle("Sage browser")).toBeNull();
  });
});
