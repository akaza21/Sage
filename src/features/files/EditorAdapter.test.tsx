import { createRef } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { EditorAdapter, type EditorAdapterHandle } from "./EditorAdapter";

afterEach(cleanup);

describe("EditorAdapter", () => {
  it("presents one controlled editor and exposes focus and selection through its narrow handle", () => {
    const onChange = vi.fn();
    const handle = createRef<EditorAdapterHandle>();
    render(
      <EditorAdapter
        ref={handle}
        ariaLabel="Edit example.ts"
        content="const answer = 42;"
        disabled={false}
        maxContentLength={1024}
        onChange={onChange}
      />,
    );

    const editor = screen.getByRole("textbox", { name: "Edit example.ts" });
    expect(screen.getAllByRole("textbox")).toHaveLength(1);
    expect(editor).toHaveValue("const answer = 42;");
    expect(editor).toHaveAttribute("maxLength", "1024");

    handle.current?.focus();
    handle.current?.setSelection(6, 12);
    expect(editor).toHaveFocus();
    expect(handle.current?.selection()).toEqual({ start: 6, end: 12 });

    fireEvent.change(editor, { target: { value: "const answer = 43;" } });
    expect(onChange).toHaveBeenCalledOnce();
    expect(onChange).toHaveBeenCalledWith("const answer = 43;");
  });
});
