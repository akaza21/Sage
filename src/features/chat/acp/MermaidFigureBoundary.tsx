import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";

const loadMermaidFigure = () => import("./MermaidFigure");

export function MermaidFigureBoundary(props: {
  source: string;
  copySource?: boolean;
  /** In a message, a card that opens the diagram in the side pane. */
  card?: boolean;
}) {
  return (
    <LazyModuleBoundary
      label="Diagram renderer"
      load={loadMermaidFigure}
      render={(MermaidFigure) => <MermaidFigure {...props} />}
    />
  );
}
