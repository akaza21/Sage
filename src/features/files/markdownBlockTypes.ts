export type MarkdownBlock =
  | { kind: "rule" }
  | { kind: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "quote"; children: readonly MarkdownBlock[] }
  | {
      kind: "list";
      items: readonly {
        children: readonly MarkdownBlock[];
        checked?: boolean;
      }[];
      ordered?: true;
      start?: number;
      loose?: true;
    }
  | { kind: "code"; text: string; language: string; fenceClosed: boolean }
  | {
      kind: "table";
      align: ("left" | "center" | "right" | undefined)[];
      rows: string[][];
    };
