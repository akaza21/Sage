import {
  useEffect,
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

const activityKindLabels = {
  root: "Harness activity",
  fork: "Branch activity",
  child: "Child activity",
  unknown: "Harness activity",
} as const;

interface ActivityItemNode {
  activity: HarnessActivityProjection;
  children: ActivityItemNode[];
  item: AcpItem;
  parentId: AcpItem["id"] | null;
}

function buildTree(
  root: HarnessActivityProjection,
  activities: readonly HarnessActivityProjection[],
  items: readonly AcpItem[],
): ActivityItemNode[] {
  const included = new Set([root.id]);
  for (let depth = 1; depth < 4; depth += 1) {
    for (const activity of activities) {
      if (
        activity.parentActivityId &&
        included.has(activity.parentActivityId)
      ) {
        included.add(activity.id);
      }
    }
  }

  const nodesByActivity = new Map<
    HarnessActivityProjection["id"],
    ActivityItemNode[]
  >();
  const nodesByItem = new Map<AcpItem["id"], ActivityItemNode>();
  for (const activity of activities) {
    if (!included.has(activity.id)) continue;
    const nodes = items
      .filter((item) => item.activityId === activity.id)
      .map((item) => ({
        activity,
        children: [],
        item,
        parentId: null,
      }));
    nodesByActivity.set(activity.id, nodes);
    for (const node of nodes) nodesByItem.set(node.item.id, node);
  }

  for (const activity of activities) {
    if (activity.id === root.id || !included.has(activity.id)) continue;
    const parent = nodesByItem.get(activity.parentItemId);
    if (!parent || parent.activity.id !== activity.parentActivityId) continue;
    for (const node of nodesByActivity.get(activity.id) ?? []) {
      node.parentId = parent.item.id;
      parent.children.push(node);
    }
  }
  return nodesByActivity.get(root.id) ?? [];
}

function flattenVisible(
  nodes: readonly ActivityItemNode[],
  expandedIds: ReadonlySet<AcpItem["id"]>,
): ActivityItemNode[] {
  const visible: ActivityItemNode[] = [];
  function visit(node: ActivityItemNode) {
    visible.push(node);
    if (expandedIds.has(node.item.id)) node.children.forEach(visit);
  }
  nodes.forEach(visit);
  return visible;
}

function flattenAll(nodes: readonly ActivityItemNode[]): ActivityItemNode[] {
  const all: ActivityItemNode[] = [];
  function visit(node: ActivityItemNode) {
    all.push(node);
    node.children.forEach(visit);
  }
  nodes.forEach(visit);
  return all;
}

export function HarnessActivity({
  activity,
  activities = [activity],
  items,
  renderItem,
}: {
  activity: HarnessActivityProjection;
  activities?: readonly HarnessActivityProjection[];
  items: readonly AcpItem[];
  renderItem?: (item: AcpItem) => ReactNode;
}) {
  const descriptionId = useId();
  const disclosureRef = useRef<HTMLElement>(null);
  const nodeRefs = useRef(new Map<AcpItem["id"], HTMLDivElement>());
  const focusedItemRef = useRef<AcpItem["id"] | null>(null);
  const restoredItemRef = useRef<AcpItem["id"] | null>(null);
  const roots = buildTree(activity, activities, items);
  const allNodes = flattenAll(roots);
  const allIds = new Set(allNodes.map((node) => node.item.id));
  const [expandedIds, setExpandedIds] = useState<Set<AcpItem["id"]>>(
    () => new Set(),
  );
  const [activeItemId, setActiveItemId] = useState<AcpItem["id"] | null>(
    roots[0]?.item.id ?? null,
  );
  const validExpandedIds = new Set(
    [...expandedIds].filter((id) => allIds.has(id)),
  );
  const visibleNodes = flattenVisible(roots, validExpandedIds);
  const activeVisible = visibleNodes.some(
    (node) => node.item.id === activeItemId,
  );
  const rovingItemId = activeVisible
    ? activeItemId
    : (visibleNodes[0]?.item.id ?? null);
  const safeLabel = activity.provenance.safeLabel ?? "Harness activity";
  const activityKind = activityKindLabels[activity.role];
  const activityLevel = Math.min(4, Math.max(1, activity.depth));
  const activeStale = activeItemId !== null && !allIds.has(activeItemId);

  useEffect(() => {
    if (!activeStale) {
      restoredItemRef.current = null;
    } else if (restoredItemRef.current !== activeItemId) {
      restoredItemRef.current = activeItemId;
      if (focusedItemRef.current === activeItemId) {
        focusedItemRef.current = null;
        disclosureRef.current?.focus();
      }
    }
  }, [activeItemId, activeStale]);

  function focusItem(index: number) {
    const node = visibleNodes[index];
    if (!node) return;
    setActiveItemId(node.item.id);
    nodeRefs.current.get(node.item.id)?.focus();
  }

  function setExpanded(itemId: AcpItem["id"], expanded: boolean) {
    setExpandedIds((current) => {
      const next = new Set([...current].filter((id) => allIds.has(id)));
      if (expanded) next.add(itemId);
      else next.delete(itemId);
      return next;
    });
  }

  function handleTreeKeyDown(
    event: KeyboardEvent<HTMLDivElement>,
    node: ActivityItemNode,
  ) {
    if (event.target !== event.currentTarget) return;
    const index = visibleNodes.findIndex(
      (candidate) => candidate.item.id === node.item.id,
    );
    if (event.key === "ArrowDown")
      focusItem(Math.min(index + 1, visibleNodes.length - 1));
    else if (event.key === "ArrowUp") focusItem(Math.max(index - 1, 0));
    else if (event.key === "Home") focusItem(0);
    else if (event.key === "End") focusItem(visibleNodes.length - 1);
    else if (event.key === "ArrowRight" && node.children.length > 0) {
      if (validExpandedIds.has(node.item.id)) focusItem(index + 1);
      else setExpanded(node.item.id, true);
    } else if (event.key === "ArrowLeft") {
      if (node.children.length > 0 && validExpandedIds.has(node.item.id)) {
        setExpanded(node.item.id, false);
      } else if (node.parentId) {
        const parentIndex = visibleNodes.findIndex(
          (candidate) => candidate.item.id === node.parentId,
        );
        focusItem(parentIndex);
      } else {
        disclosureRef.current?.focus();
      }
    } else if (event.key === "Enter") {
      const action = [
        ...event.currentTarget.querySelectorAll<HTMLButtonElement>(
          ".acp-resource-action:not(:disabled)",
        ),
      ].find((candidate) => {
        const details = candidate.closest("details");
        return !details || details.open;
      });
      action?.click();
    } else return;
    event.preventDefault();
  }

  return (
    <details
      className="acp-activity acp-child-activity"
      role="group"
      aria-label={`Harness activity: ${safeLabel}`}
      aria-describedby={descriptionId}
      data-sage-activity-id={activity.id}
    >
      <summary className="acp-child-activity__summary" ref={disclosureRef}>
        <span className="acp-child-activity__identity">
          <span className="acp-child-activity__kind">{activityKind}</span>
          <span className="acp-child-activity__label" title={safeLabel}>
            {safeLabel}
          </span>
        </span>
        <span
          className="acp-state acp-child-activity__status"
          role="status"
          aria-label={lifecycleLabels[activity.lifecycle]}
          aria-live="off"
          data-lifecycle={activity.lifecycle}
        >
          {lifecycleLabels[activity.lifecycle]}
        </span>
      </summary>
      <span className="sr-only" id={descriptionId}>
        Bounded read-only child activity in the originating turn
      </span>
      <p className="acp-activity__provenance">
        Harness-owned {activity.role} activity · level {activityLevel}
      </p>
      <div
        className="acp-child-activity__tree"
        role="tree"
        aria-label={`${safeLabel} activity items`}
      >
        {visibleNodes.map((node) => (
          <div
            key={node.item.id}
            className="acp-child-activity__item"
            ref={(element) => {
              if (element) nodeRefs.current.set(node.item.id, element);
              else nodeRefs.current.delete(node.item.id);
            }}
            role="treeitem"
            aria-level={Math.min(4, Math.max(1, node.activity.depth))}
            aria-expanded={
              node.children.length > 0
                ? validExpandedIds.has(node.item.id)
                : undefined
            }
            tabIndex={node.item.id === rovingItemId ? 0 : -1}
            data-sage-id={node.item.id}
            onFocus={(event) => {
              if (event.target === event.currentTarget)
                setActiveItemId(node.item.id);
            }}
            onFocusCapture={() => {
              setActiveItemId(node.item.id);
              focusedItemRef.current = node.item.id;
            }}
            onBlurCapture={(event) => {
              if (
                event.relatedTarget &&
                !event.currentTarget.contains(event.relatedTarget as Node)
              ) {
                focusedItemRef.current = null;
              }
            }}
            onKeyDown={(event) => handleTreeKeyDown(event, node)}
          >
            {renderItem ? (
              renderItem(node.item)
            ) : node.item.kind === "collaboration" ? (
              <CollaborationItem item={node.item} />
            ) : node.item.kind === "unsupported" ? (
              <UnknownItem item={node.item} />
            ) : null}
          </div>
        ))}
      </div>
    </details>
  );
}
