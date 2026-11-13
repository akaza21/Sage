export const tableAnswer = [
  "## How far from the chat goal",
  "",
  "| Work | Status | Next |",
  "| :--- | :---: | ---: |",
  "| **Harnesses** | Shared rendering | Check native evidence |",
  "| Inline code | `capture-recorder.ps1` | Retained source |",
  "| Escaped pipe | a\\|b | *bounded* |",
  "",
  "- **Harnesses:** Codex, Claude Code and OpenCode share the same answer renderer.",
  "- **Pi:** text-only; no fabricated image controls.",
  "",
  "1. Database comparison",
  "2. Basic email format validator",
].join("\n");

export const pythonAnswer = [
  "| Database | Best suited for | Architecture |",
  "| --- | --- | --- |",
  "| SQLite | Local apps and lightweight transactions | Embedded; one writer at a time |",
  "| Postgres | Multi-user applications and concurrent transactions | Client-server; many concurrent writers |",
  "| DuckDB | Analytical queries over large datasets | Embedded column-oriented execution |",
  "",
  "3. Basic email format validator",
  "",
  "```python",
  "import re",
  "",
  "def valid_email(value: str) -> bool:",
  "    # Basic format check; does not verify mailbox existence.",
  '    return bool(re.fullmatch(r"[^\\s@]+@[^\\s@]+\\.[^\\s@]+", value))',
  "```",
].join("\n");

export const unlabelledAnswer = `The subagent replied with this output:\n\n\`\`\`\nCP18D_CHILD_OK ${"retained-source-".repeat(36)}\n\`\`\`\n\nOnly the requested command ran.`;

export const flowchartSource = [
  "flowchart TD",
  "A[Request password reset] --> B[Return generic acknowledgment]",
  "B --> C[Email expiring single-use link if account exists]",
  "C --> D[Open link and enter new password]",
  "D --> E{Token and password valid?}",
  "E -->|Yes| F[Hash new password and consume token atomically]",
  "E -->|No| A",
].join("\n");

export const cp22MarkdownAnswers = {
  table: tableAnswer,
  python: pythonAnswer,
  unlabelled: unlabelledAnswer,
  passwordReset: `1. Password reset\n\n\`\`\`mermaid\n${flowchartSource}\n\`\`\``,
  streaming: "```mermaid\ngraph TD\nA -->",
  fallback: "```mermaid\nunsupported diagram\n```",
  combined: `${tableAnswer}\n\n${pythonAnswer}\n\n\`\`\`mermaid\n${flowchartSource}\n\`\`\``,
};
