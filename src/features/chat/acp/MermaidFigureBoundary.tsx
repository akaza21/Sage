import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";

const loadMermaidFigure = () => import("./MermaidFigure");

export function MermaidFigureBoundary(props: {
  source: string;
  copySource?: boolean;
}) {
  return (
    <LazyModuleBoundary
      label="Diagram renderer"
      load={loadMermaidFigure}
      render={(MermaidFigure) => <MermaidFigure {...props} />}
    />
  );
}
