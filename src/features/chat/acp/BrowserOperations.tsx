import type { BrowserOperationProjection } from "../../../domain/browserOperation";
import { useContext } from "react";
import { WorkedActivityVisibility } from "./WorkedActivityVisibility";
import { BrowserOperationItem } from "./BrowserOperationItem";

const MAX_VISIBLE_BROWSER_OPERATIONS = 32;

export function BrowserOperations({
  operations,
  collapsed,
}: {
  operations: readonly BrowserOperationProjection[];
  collapsed: number;
}) {
  const expanded = useContext(WorkedActivityVisibility);
  if (!expanded) return null;
  const visible = operations.slice(-MAX_VISIBLE_BROWSER_OPERATIONS);
  const earlier = Math.max(0, collapsed) + operations.length - visible.length;
  if (visible.length === 0 && earlier === 0) return null;
  return (
    <section className="acp-browser-operations" aria-label="Browser actions">
      <h3 className="sr-only">Browser actions</h3>
      {earlier > 0 ? (
        <p className="acp-browser-operations__earlier">
          +{earlier} earlier browser action{earlier === 1 ? "" : "s"}
        </p>
      ) : null}
      {visible.map((operation) => (
        <BrowserOperationItem
          key={operation.operationId}
          operation={operation}
        />
      ))}
    </section>
  );
}
