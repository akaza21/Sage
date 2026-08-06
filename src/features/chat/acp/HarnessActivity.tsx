import {
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";

import type { AcpItem, HarnessActivityProjection } from "../../../domain/acp";

import { CollaborationItem } from "./CollaborationItem";
import { UnknownItem } from "./UnknownItem";

const lifecycleLabels = {
  observed: "Observed",
  active: "Active",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
  unsupported: "Unsupported",
} as const;

export function HarnessActivity({
  activity,
  items,
  renderItem,
}: {
  activity: HarnessActivityProjection;
  items: readonly AcpItem[];
  renderItem?: (item: AcpItem) => ReactNode;
}) {
  const ownedItems = items.filter((item) => item.activityId === activity.id);
  const descriptionId = useId();
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const disclosureRef = useRef<HTMLElement>(null);
  const nodeRefs = useRef(new Map<AcpItem["id"], HTMLDivElement>());
  const [activeItemId, setActiveItemId] = useState(ownedItems[0]?.id ?? null);
  const retainedActiveItemId = ownedItems.some(
    (item) => item.id === activeItemId,
  )
    ? activeItemId
    : null;
  const safeLabel = activity.provenance.safeLabel ?? "Harness activity";
  const level = Math.min(4, Math.max(1, activity.depth));

  function focusItem(index: number) {
    const item = ownedItems[index];
    if (!item) return;
    setActiveItemId(item.id);
    nodeRefs.current.get(item.id)?.focus();
  }

  function ownNode(item: AcpItem, node: HTMLDivElement | null) {
    if (!node) {
      nodeRefs.current.delete(item.id);
      return;
    }
    nodeRefs.current.set(item.id, node);
    node
      .querySelectorAll<HTMLElement>(
        "a[href], button, input, select, summary, textarea, [tabindex]",
      )
      .forEach((descendant) => {
        descendant.tabIndex = -1;
      });
  }

  function handleTreeKeyDown(
    event: KeyboardEvent<HTMLDivElement>,
    item: AcpItem,
  ) {
    const index = ownedItems.findIndex((candidate) => candidate.id === item.id);
    if (event.key === "ArrowDown")
      focusItem(Math.min(index + 1, ownedItems.length - 1));
    else if (event.key === "ArrowUp") focusItem(Math.max(index - 1, 0));
    else if (event.key === "Home") focusItem(0);
    else if (event.key === "End") focusItem(ownedItems.length - 1);
    else if (event.key === "ArrowLeft") {
      if (detailsRef.current) detailsRef.current.open = false;
      disclosureRef.current?.focus();
    } else if (event.key === "Enter") {
      event.currentTarget
        .querySelector<HTMLButtonElement>(".acp-resource-action:not(:disabled)")
        ?.click();
    } else return;
    event.preventDefault();
  }

  return (
    <details
      ref={detailsRef}
      className="acp-activity"
      role="group"
      aria-label={`Harness activity: ${safeLabel}`}
      aria-describedby={descriptionId}
    >
      <summary ref={disclosureRef}>
        <span>{safeLabel}</span>
        <span
          className="acp-state"
          role="status"
          aria-label={lifecycleLabels[activity.lifecycle]}
        >
          {lifecycleLabels[activity.lifecycle]}
        </span>
      </summary>
      <span className="sr-only" id={descriptionId}>
        Bounded read-only child activity in the originating Turn
      </span>
      <p className="acp-activity__provenance">
        Harness-owned {activity.role} activity · depth {activity.depth}
      </p>
      <div role="tree" aria-label={`${safeLabel} activity items`}>
        {ownedItems.map((item) => (
          <div
            key={item.id}
            ref={(node) => ownNode(item, node)}
            role="treeitem"
            aria-level={level}
            tabIndex={item.id === retainedActiveItemId ? 0 : -1}
            data-sage-id={item.id}
            onFocus={() => setActiveItemId(item.id)}
            onKeyDown={(event) => handleTreeKeyDown(event, item)}
          >
            {renderItem ? (
              renderItem(item)
            ) : item.kind === "collaboration" ? (
              <CollaborationItem item={item} />
            ) : item.kind === "unsupported" ? (
              <UnknownItem item={item} />
            ) : null}
          </div>
        ))}
      </div>
    </details>
  );
}
