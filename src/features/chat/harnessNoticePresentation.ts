import type { ComponentType, ReactNode } from "react";
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
