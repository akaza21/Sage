import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

/** Clamps a long prompt to six lines with a fade and an Expand button. */
export function UserPromptClamp({ children }: { children: ReactNode }) {
  const frame = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const [overflows, setOverflows] = useState(false);
  const [expanded, setExpanded] = useState(false);
  useLayoutEffect(() => {
    const outer = frame.current;
    const inner = body.current;
    if (!outer || !inner || expanded) return;
    // The body keeps its natural height, so it resizes as lazy prose lands.
    const measure = () =>
      setOverflows(inner.scrollHeight > outer.clientHeight + 1);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(inner);
    return () => observer.disconnect();
  }, [expanded]);
  return (
    <>
      <div
        ref={frame}
        className="acp-message__clamp"
        data-expanded={expanded || undefined}
        data-clamped={(overflows && !expanded) || undefined}
      >
        <div ref={body}>{children}</div>
      </div>
      {overflows ? (
        <button
          type="button"
          className="acp-message__expand"
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
        >
          {expanded ? "Collapse" : "Expand"}
        </button>
      ) : null}
    </>
  );
}
