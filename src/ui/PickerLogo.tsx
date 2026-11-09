/** Official marks use external path data; custom and unavailable marks keep their existing assets. */
export function PickerLogo({ logo, icon }: { logo?: string; icon?: string }) {
  const builtin = `/harness-${icon}.svg`;
  const src = logo ?? builtin;
  return src === builtin ? (
    <svg
      className="harness-icon"
      viewBox="0 0 24 24"
      aria-hidden="true"
      focusable="false"
      data-harness-icon={icon}
    >
      <use href={`${src}#mark`} />
    </svg>
  ) : (
    <img
      src={src}
      alt=""
      aria-hidden="true"
      data-harness-icon={icon ?? "generic"}
    />
  );
}
