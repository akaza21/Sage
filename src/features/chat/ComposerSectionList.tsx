import { Fragment, useState } from "react";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";
import type { ComposerMenuSection } from "./composerMenuSections";
import { composerMenuRow } from "./composerMenuRow";

export function ComposerMenuSections({
  sections,
  disabled,
  onPick,
}: {
  sections: readonly ComposerMenuSection[];
  disabled: boolean;
  onPick: (command: AvailableAcpCommand, input: string) => void;
}) {
  return sections.map((section) => (
    <div key={section.id} role="group" aria-label={section.label}>
      <p className="menu-group-label composer__add-heading" aria-hidden="true">
        {section.label}
      </p>
      {section.groups.map((group, index) => (
        <div
          key={group.label ?? index}
          role={group.label ? "group" : undefined}
          aria-label={group.label}
        >
          {group.label ? (
            <p className="menu-group-label" aria-hidden="true">
              {group.label}
            </p>
          ) : null}
          {group.rows.map((row) => (
            <Fragment key={`${row.kind}:${row.id}`}>
              {row.kind === "native" ? (
                row.content
              ) : (
                <CommandRow
                  command={row.command}
                  disabled={disabled}
                  onPick={onPick}
                />
              )}
            </Fragment>
          ))}
        </div>
      ))}
      {section.more ? (
        <p className="menu-empty">and {section.more} more, type to search</p>
      ) : null}
    </div>
  ));
}

function CommandRow({
  command,
  disabled,
  onPick,
}: {
  command: AvailableAcpCommand;
  disabled: boolean;
  onPick: (command: AvailableAcpCommand, input: string) => void;
}) {
  const [input, setInput] = useState("");
  return (
    <div
      className="composer-menu__command"
      onKeyDown={(event) => {
        if (
          event.key === "Tab" &&
          !event.shiftKey &&
          command.inputHint &&
          event.target instanceof HTMLButtonElement
        ) {
          event.preventDefault();
          event.stopPropagation();
          event.currentTarget.querySelector("input")?.focus();
        }
      }}
    >
      {composerMenuRow(
        {
          disabled,
          "aria-label": `/${command.name}`,
          onClick: () => onPick(command, input),
        },
        `/${command.name}`,
        command.description ?? undefined,
      )}
      {command.inputHint ? (
        <input
          type="text"
          aria-label={`Arguments for /${command.name}`}
          placeholder={command.inputHint}
          maxLength={4096}
          value={input}
          disabled={disabled}
          onChange={(event) => setInput(event.target.value.slice(0, 4096))}
          onKeyDown={(event) => {
            if (event.key === "Escape" || event.key === "Tab") return;
            event.stopPropagation();
            if (event.key === "Enter" && !event.nativeEvent.isComposing) {
              event.preventDefault();
              onPick(command, input);
            }
          }}
        />
      ) : null}
    </div>
  );
}
