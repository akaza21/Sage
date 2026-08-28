import type { MermaidStateSource } from "./mermaidState";

export function StateSvg({
  diagram,
}: {
  diagram: Extract<MermaidStateSource, { ok: true }>;
}) {
  const column = 120;
  const width = Math.max(diagram.states.length * column, 160);
  const height = 72 + Math.max(diagram.transitions.length, 1) * 36;
  return (
    <svg
      role="img"
      aria-label="Mermaid state diagram"
      viewBox={`0 0 ${width} ${height}`}
      width="100%"
      height={Math.min(height, 280)}
    >
      {diagram.states.map((state, index) => {
        const x = index * column + 60;
        return (
          <g key={`${state.id}:${index}`}>
            {state.terminal ? (
              <circle cx={x} cy={22} r={8} fill="currentColor" />
            ) : (
              <rect
                x={x - 44}
                y={8}
                width={88}
                height={28}
                rx={14}
                fill="var(--surface-raised)"
                stroke="currentColor"
              />
            )}
            {state.terminal ? null : (
              <text
                x={x}
                y={26}
                textAnchor="middle"
                fill="currentColor"
                fontSize="12"
              >
                {state.id}
              </text>
            )}
          </g>
        );
      })}
      {diagram.transitions.map((transition, index) => {
        const from = diagram.states.findIndex(
          (state) => state.id === transition.from,
        );
        const to = diagram.states.findIndex(
          (state) => state.id === transition.to,
        );
        if (from < 0 || to < 0) return null;
        const y = 56 + index * 36;
        const x1 = from * column + 60;
        const x2 = to * column + 60;
        return (
          <g key={`${transition.from}:${transition.to}:${index}`}>
            <line x1={x1} y1={y} x2={x2} y2={y} stroke="currentColor" />
            {transition.label ? (
              <text
                x={(x1 + x2) / 2}
                y={y - 6}
                textAnchor="middle"
                fill="currentColor"
                fontSize="11"
              >
                {transition.label}
              </text>
            ) : null}
          </g>
        );
      })}
    </svg>
  );
}
