import { createElement, forwardRef, memo } from "react";
import type { LucideProps } from "lucide-react";

type IconNode = string | readonly [string, Record<string, string>];

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
          {nodes.map((node, index) =>
            typeof node === "string"
              ? createElement("path", { d: node, key: index })
              : createElement(node[0], { ...node[1], key: index }),
          )}
          {children}
        </svg>
      );
    },
  );
  return memo(Component);
}

export function lucideIcon(name: string, nodes: readonly IconNode[]) {
  return leanIcon(`lucide lucide-${name}`, nodes);
}

/** Sage's outline leaf, shared by launch, Editor welcome and new Chat. */
export function SageMark({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      width="40"
      height="40"
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
      focusable="false"
    >
      <use href="/sage-mark.svg#mark" />
    </svg>
  );
}
