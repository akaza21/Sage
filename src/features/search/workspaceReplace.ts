// Workspace Replace's shapes. What previews and applies a Replace lives in
// `workspaceReplacePlan`, which loads on the first Replace.

import type { EditorDocument, ResourceRef } from "../../domain/workbench";

export interface ReplaceSpec {
  query: string;
  replacement: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  useRegex: boolean;
  preserveCase: boolean;
}

export interface MatchPosition {
  lineNumber: number;
  columnNumber: number;
}

export interface ReplaceLinePreview extends MatchPosition {
  before: string;
  after: string;
}

export type ReplaceProblemKind = "stale" | "refused" | "noMatches";

export interface ReplaceFileProblem {
  resource: ResourceRef;
  kind: ReplaceProblemKind;
  message: string;
}

export interface ReplaceFilePreview {
  resource: ResourceRef;
  fingerprint: string;
  count: number;
  lines: ReplaceLinePreview[];
  linesTruncated: boolean;
}

export interface TextReplaceResult {
  content: string;
  count: number;
  lines: ReplaceLinePreview[];
  linesTruncated: boolean;
}

export interface WorkspaceReplaceGateway {
  preview(request: {
    projectId: string;
    expectedTransitionEpoch: number;
    spec: ReplaceSpec;
    resources: ResourceRef[];
    only?: MatchPosition;
  }): Promise<{
    files: ReplaceFilePreview[];
    problems: ReplaceFileProblem[];
    truncated: boolean;
  }>;
  apply(request: {
    projectId: string;
    expectedTransitionEpoch: number;
    spec: ReplaceSpec;
    files: {
      resource: ResourceRef;
      fingerprint: string;
      only?: MatchPosition;
    }[];
  }): Promise<{
    replaced: { resource: ResourceRef; count: number; fingerprint: string }[];
    problems: ReplaceFileProblem[];
  }>;
  replaceText(request: {
    spec: ReplaceSpec;
    text: string;
    only?: MatchPosition;
  }): Promise<TextReplaceResult>;
}

/** Where a planned File's change goes. */
export type ReplaceTarget =
  /** On disk, natively. */
  | { kind: "disk"; fingerprint: string }
  /** In the editor showing the File, as one undoable edit. */
  | { kind: "editor"; text: string; content: string }
  /** In the unsaved buffer of a File open in another tab. */
  | { kind: "buffer"; document: EditorDocument; content: string };

export interface PlannedFile {
  resource: ResourceRef;
  count: number;
  lines: ReplaceLinePreview[];
  linesTruncated: boolean;
  target: ReplaceTarget;
}

export interface ReplacePlan {
  projectId: string;
  transitionEpoch: number;
  spec: ReplaceSpec;
  /** Set when the Replace is of one match. */
  only?: MatchPosition;
  files: PlannedFile[];
  problems: ReplaceFileProblem[];
  truncated: boolean;
  matchCount: number;
}

export interface ReplaceOutcome {
  matchCount: number;
  fileCount: number;
  problems: ReplaceFileProblem[];
}
