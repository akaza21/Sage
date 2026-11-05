import {
  useEffect,
  useImperativeHandle,
  useState,
  type RefObject,
} from "react";
import { X } from "../../ui/leanIcons";
import { contentText, type EditorContent } from "./editorContent";
import { goToLineSelection } from "./goToLine";

export interface FileGoToLineBarProps {
  query: string;
  pendingSubmitRef: RefObject<boolean>;
  submitRef: RefObject<(() => void) | null>;
  content: EditorContent;
  onSubmit: (selection: { start: number; end: number }) => void;
  onClose: () => void;
}

export default function FileGoToLineBar({
  query,
  pendingSubmitRef,
  submitRef,
  content,
  onSubmit,
  onClose,
}: FileGoToLineBarProps) {
  const [invalidQuery, setInvalidQuery] = useState<string>();
  useImperativeHandle(
    submitRef,
    () => () => {
      pendingSubmitRef.current = false;
      const selection = goToLineSelection(contentText(content), query);
      if (selection) onSubmit(selection);
      else setInvalidQuery(query);
    },
    [pendingSubmitRef, content, query, onSubmit],
  );
  useEffect(() => {
    // Both direct and buffered submits consume the pending flag before navigation.
    if (pendingSubmitRef.current) submitRef.current?.();
  }, [pendingSubmitRef, submitRef]);
  if (invalidQuery !== undefined && invalidQuery !== query)
    setInvalidQuery(undefined);
  return (
    <>
      <span className="editor-find__status" role="status">
        {invalidQuery === query ? "Enter a line" : "Line or line:column"}
      </span>
      <button
        className="icon-button editor-find__button"
        type="button"
        aria-label="Close go to line"
        onClick={onClose}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </>
  );
}
