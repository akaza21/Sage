import { useCallback, useRef, useState } from "react";
import { ShieldCheck } from "../../../ui/leanIcons";

import type { AcpItem, PermissionDecision } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { getPermissionStatus } from "./statusSemantics";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

export function PermissionItem({ item }: { item: PermissionProjectionItem }) {
  const status = getPermissionStatus(item);
  const [resolving, setResolving] = useState(false);
  const [resolutionError, setResolutionError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const awaitingDecision =
    item.toolState === "waitingForApproval" &&
    (item.payload.state === "received" || item.payload.state === "asked");
  const browserPermission = item.payload.source === "browser";
  const offered = item.payload.offeredOptionKinds;
  const canAllowOnce =
    browserPermission || offered?.includes("allow_once") === true;
  const canAllowAlways =
    !browserPermission && offered?.includes("allow_always") === true;
  const canDeny =
    browserPermission ||
    !offered ||
    offered.includes("reject_once") ||
    offered.includes("reject_always");

  const resolve = useCallback(
    (decision: PermissionDecision) => {
      if (
        !awaitingDecision ||
        inFlight.current ||
        (decision === "allowOnce" && !canAllowOnce) ||
        (decision === "allowAlways" && !canAllowAlways) ||
        (decision === "deny" && !canDeny) ||
        (decision === "allowForOriginSession" && !browserPermission)
      ) {
        return;
      }
      inFlight.current = true;
      setResolving(true);
      setResolutionError(null);
      void acpDesktop
        .resolvePermissionDecision({
          projectId: item.projectId,
          chatId: item.chatId,
          sessionId: item.sessionId,
          turnId: item.turnId,
          runtimeEpoch: item.runtimeEpoch,
          requestId: item.payload.requestId,
          decision,
        })
        .catch(() => {
          inFlight.current = false;
          setResolving(false);
          setResolutionError(
            "This permission action could not be completed. You can try Deny or cancel the Turn.",
          );
        });
    },
    [
      awaitingDecision,
      browserPermission,
      canAllowOnce,
      canAllowAlways,
      canDeny,
      item,
    ],
  );

  if (!awaitingDecision) {
    return (
      <article
        className="acp-permission acp-permission--settled"
        role="group"
        aria-label={
          browserPermission ? "Browser permission" : "Permission request"
        }
        data-activity-state={item.toolState}
        data-acp-item-id={item.id}
      >
        <details>
          <summary>
            <ShieldCheck size={14} aria-hidden="true" />
            <span className="acp-permission__label">{item.payload.label}</span>
            <span aria-hidden="true"> · </span>
            <span
              className="acp-permission__status"
              role="status"
              aria-label={status.accessible}
              aria-live="off"
              aria-atomic="true"
            >
              {status.visual}
            </span>
          </summary>
          <p>{item.payload.summary}</p>
        </details>
      </article>
    );
  }

  return (
    <article
      className="acp-permission"
      role="group"
      aria-label={
        browserPermission ? "Browser permission" : "Permission request"
      }
      data-activity-state={item.toolState}
      data-acp-item-id={item.id}
      tabIndex={awaitingDecision ? 0 : undefined}
      onKeyDown={
        awaitingDecision
          ? (event) => {
              if (event.target !== event.currentTarget) {
                return;
              }
              if (event.key === "Enter" && canAllowOnce) {
                event.preventDefault();
                resolve("allowOnce");
              } else if (event.key === "Escape" && canDeny) {
                event.preventDefault();
                resolve("deny");
              }
            }
          : undefined
      }
    >
      <header>
        <strong>{item.payload.label}</strong>
        <span
          role="status"
          aria-label={status.accessible}
          aria-live="off"
          aria-atomic="true"
        >
          {status.visual}
        </span>
      </header>
      <p>{item.payload.summary}</p>
      {resolutionError ? (
        <p className="acp-permission__error" role="alert">
          {resolutionError}
        </p>
      ) : null}
      {awaitingDecision ? (
        <div className="acp-permission__actions">
          {browserPermission ? (
            <button
              type="button"
              className="acp-permission__action"
              disabled={resolving}
              onClick={() => resolve("allowOnce")}
            >
              Allow once
            </button>
          ) : (
            offered
              ?.filter((kind, index) => offered.indexOf(kind) === index)
              .map((kind) => {
                if (kind !== "allow_once" && kind !== "allow_always") {
                  return null;
                }
                const decision =
                  kind === "allow_once" ? "allowOnce" : "allowAlways";
                return (
                  <button
                    key={kind}
                    type="button"
                    className="acp-permission__action"
                    disabled={resolving}
                    onClick={() => resolve(decision)}
                  >
                    {kind === "allow_once" ? "Approve" : "Always allow"}
                  </button>
                );
              })
          )}
          {browserPermission ? (
            <button
              type="button"
              className="acp-permission__action"
              disabled={resolving}
              onClick={() => resolve("allowForOriginSession")}
            >
              Allow for this tab and origin
            </button>
          ) : null}
          {canDeny ? (
            <button
              type="button"
              className="acp-permission__action"
              disabled={resolving}
              onClick={() => resolve("deny")}
            >
              Deny
            </button>
          ) : null}
        </div>
      ) : null}
    </article>
  );
}
