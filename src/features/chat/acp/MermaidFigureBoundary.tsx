import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";

const loadMermaidFigure = () =>
  import("./MermaidFigure").then(({ MermaidFigure }) => ({
    default: MermaidFigure,
  }));

export function MermaidFigureBoundary({ source }: { source: string }) {
  return (
    <LazyModuleBoundary
      label="Diagram renderer"
      load={loadMermaidFigure}
      render={(MermaidFigure) => <MermaidFigure source={source} />}
    />
  );
}
