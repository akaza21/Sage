import { createElement, forwardRef } from "react";
import type { LucideProps } from "lucide-react";

type IconNode = readonly [string, Record<string, string>];

export function leanIcon(cssClass: string, nodes: readonly IconNode[]) {
  const Component = forwardRef<SVGSVGElement, LucideProps>(
    (
      {
        color = "currentColor",
        size = 24,
        strokeWidth = 2,
        absoluteStrokeWidth = false,
        className,
        children,
        ...rest
      },
      ref,
    ) => {
      const hasA11yProp = Object.keys(rest).some(
        (key) => key.startsWith("aria-") || key === "role" || key === "title",
      );
      return (
        <svg
          ref={ref}
          xmlns="http://www.w3.org/2000/svg"
          width={size}
          height={size}
          viewBox="0 0 24 24"
          fill="none"
          stroke={color}
          strokeWidth={
            absoluteStrokeWidth
              ? (Number(strokeWidth) * 24) / Number(size)
              : strokeWidth
          }
          strokeLinecap="round"
          strokeLinejoin="round"
          className={`${cssClass}${className ? ` ${className}` : ""}`}
          {...(!children && !hasA11yProp ? { "aria-hidden": "true" } : {})}
          {...rest}
        >
          {nodes.map(([tag, attributes], index) =>
            createElement(tag, { ...attributes, key: index }),
          )}
          {children}
        </svg>
      );
    },
  );
  return Component;
}
