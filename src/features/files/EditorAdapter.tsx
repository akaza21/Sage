import {
  forwardRef,
  useImperativeHandle,
  useRef,
  type ChangeEvent,
} from "react";

export interface EditorSelection {
  start: number;
  end: number;
}

export interface EditorAdapterHandle {
  focus: () => void;
  selection: () => EditorSelection;
  setSelection: (start: number, end: number) => void;
}

interface EditorAdapterProps {
  ariaLabel: string;
  content: string;
  disabled: boolean;
  maxContentLength: number;
  onChange: (content: string) => void;
}

export const EditorAdapter = forwardRef<
  EditorAdapterHandle,
  EditorAdapterProps
>(function EditorAdapter(
  { ariaLabel, content, disabled, maxContentLength, onChange },
  ref,
) {
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useImperativeHandle(
    ref,
    () => ({
      focus: () => inputRef.current?.focus(),
      selection: () => ({
        start: inputRef.current?.selectionStart ?? 0,
        end: inputRef.current?.selectionEnd ?? 0,
      }),
      setSelection: (start, end) =>
        inputRef.current?.setSelectionRange(start, end),
    }),
    [],
  );

  const change = (event: ChangeEvent<HTMLTextAreaElement>) => {
    onChange(event.target.value);
  };

  return (
    <textarea
      ref={inputRef}
      className="editor-input"
      aria-label={ariaLabel}
      value={content}
      maxLength={maxContentLength}
      disabled={disabled}
      spellCheck={false}
      onChange={change}
    />
  );
});
