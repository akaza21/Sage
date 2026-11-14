import "./tooltip.css";
import { installTooltipLayer } from "./tooltipLayer";

const uninstall = installTooltipLayer();
import.meta.hot?.dispose(uninstall);
