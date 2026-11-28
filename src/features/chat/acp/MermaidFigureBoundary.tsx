import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";

const loadMermaidFigure = () => import("./MermaidFigure");

export function MermaidFigureBoundary(props: {
  source: string;
  copySource?: boolean;
  /** A Chat's diagram renders as a card that opens in its side pane. */
  chatId?: string;
}) {
  return (
    <LazyModuleBoundary
      label="Diagram renderer"
      load={loadMermaidFigure}
      render={(MermaidFigure) => <MermaidFigure {...props} />}
    />
  );
}
