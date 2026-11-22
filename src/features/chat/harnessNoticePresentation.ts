import type { ComponentType, Dispatch, ReactNode, SetStateAction } from "react";
import type { HarnessDefinition } from "../../domain/harness";
import type { NoticeProps } from "../../ui/Notice";
export interface HarnessNoticePresentationProps {
  harness: HarnessDefinition | null | undefined;
  binding?: NoticeProps;
  /** Only the current compatibility fact and release, never a history. */
  builds?: readonly NoticeProps[];
  discovery?: NoticeProps;
  actions?: ReactNode;
}

export type HarnessNoticePresentation =
  ComponentType<HarnessNoticePresentationProps>;

/** One compatibility identity and one release identity, never a history. */
type DismissalIdentities = { build?: string; release?: string };
export type HarnessNoticeDismissal = [
  DismissalIdentities,
  Dispatch<SetStateAction<DismissalIdentities>>,
];
