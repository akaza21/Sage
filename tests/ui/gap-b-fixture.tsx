import { createRoot } from "react-dom/client";
import { useRef, useState } from "react";
import { ChatComposerRegion } from "../../src/features/chat/ChatComposerRegion";
import { ChatHome } from "../../src/features/chat/ChatHome";
import { ComposerModelControls } from "../../src/features/chat/ComposerModelControls";
import { ChatCodeBlock } from "../../src/features/chat/acp/ChatCodeBlock";
import { ItemView } from "../../src/features/chat/acp/TurnActivityItem";
import { TurnFileActivity } from "../../src/features/chat/acp/TurnFileActivity";
import { snapshotPage } from "../../src/app/agentDiff.fixture";
import { textItem } from "../../src/features/chat/acp/acpProjection.fixture";
import { TurnDaySeparator } from "../../src/features/chat/acp/AcpTurnDates";
import type { ImageAttachedContext } from "../../src/domain/attachedContext";
import "../../src/styles/global.css";
import "../../src/styles/shared-controls.css";
import "../../src/styles/shared-dialogs.css";
import "../../src/styles/chat-surface.css";
import "../../src/styles/composer-surface.css";
import "../../src/styles/composer-model-picker.css";
import "../../src/styles/composer-image.css";
import "../../src/styles/chat-container-controls.css";
import "../../src/features/chat/acp/acp-chat.css";

const image: ImageAttachedContext = {
  kind: "image",
  artifactId: "00000000-0000-4000-8000-000000000000",
  mediaType: "image/png",
  width: 1,
  height: 1,
  byteLength: 1,
  sha256: "a".repeat(64),
  label: "attached.png",
};
const thumbnail =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a/0sAAAAASUVORK5CYII=";
const projectName =
  "A very long project name with readable workspace context at a narrow width";
const root = createRoot(document.getElementById("root")!);
export function Fixture({ surface }: { surface: string }) {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const [draft, setDraft] = useState("");
  const [submitted, setSubmitted] = useState("");
  const [selected, setSelected] = useState("model-204");
  const [modelsOpen, setModelsOpen] = useState(false);
  const modelTrigger = useRef<HTMLButtonElement>(null);
  const [opened, setOpened] = useState("");
  const saved = snapshotPage();
  return (
    <section
      className="chat-surface message-list__content"
      style={{ width: "100%", display: "block", padding: "var(--space-3)" }}
    >
      {surface === "home" ? <ChatHome projectName={projectName} /> : null}
      {surface === "models" ? (
        <>
          <ComposerModelControls
            triggerRef={modelTrigger}
            panelId="models"
            open={modelsOpen}
            effortOpen={false}
            onToggle={() => setModelsOpen((open) => !open)}
            onToggleEffort={() => {}}
            onDismiss={() => setModelsOpen(false)}
            choices={Array.from({ length: 205 }, (_, i) => ({
              id: `model-${i}`,
              name: `Model ${i}`,
            }))}
            selectedId={selected}
            modelLabel="Model"
            effortLabel={null}
            contextWindow={null}
            effortOption={undefined}
            disabled={false}
            pending={false}
            onSelectModel={(id) => {
              setSelected(id);
              setModelsOpen(false);
            }}
            onSetOption={async () => {}}
          />
          <output aria-label="Selected model">{selected}</output>
        </>
      ) : null}
      {surface === "code" ? (
        <ChatCodeBlock
          language="unknown"
          source={"  " + "long source ".repeat(120) + "\n\tend\n"}
        />
      ) : null}
      {surface === "user" ? (
        <>
          <TurnDaySeparator timestamp={textItem().createdAt} />
          <ItemView
            item={{
              ...textItem(),
              payload: {
                role: "user",
                text: "**Exact** multiline\nprompt `source`",
              },
            }}
            interactive
          />
        </>
      ) : null}
      {surface === "files" ? (
        <>
          <TurnFileActivity
            page={saved}
            turn={saved.turns[0]}
            onOpenResource={(resource, target) =>
              setOpened(`${target}:${resource.relativePath}`)
            }
          />
          <output aria-label="Opened resource">{opened}</output>
        </>
      ) : null}
      {surface === "composer" ? (
        <>
          <ChatComposerRegion
            composerRef={composerRef}
            fieldId="prompt"
            dictationScope="chat-a"
            descriptionId="description"
            disabledReasonId="disabled"
            text={draft}
            attachedContext={[image]}
            imagePreviews={{ [image.artifactId]: thumbnail }}
            imageInputAvailable
            enabled
            actionBusy={false}
            activeTurn={false}
            projectTransitioning={false}
            workbenchError={null}
            chatError={null}
            availableBinding
            canPrompt
            discoveryPhase="ready"
            sessionHarnessId={null}
            sessionModelId={null}
            selectedHarnessId={null}
            selectedModelId={null}
            sessionPromptUnavailable={false}
            sessionConfig={null}
            modePending={false}
            modeError={null}
            focused
            onSetSessionMode={async () => {}}
            onSetSessionConfigOption={async () => {}}
            onSelectHarness={async () => {}}
            onSelectModel={() => {}}
            missingBindingNotice={false}
            disabledReason={null}
            project={{ name: projectName }}
            usage={null}
            goal={{ kind: "unsupported" }}
            onSubmit={(event) => {
              event.preventDefault();
              setSubmitted(draft);
            }}
            onTextChange={setDraft}
            onRemoveContext={() => {}}
            onCancelTurn={null}
          />
          <output aria-label="Submitted text">{submitted}</output>
        </>
      ) : null}
    </section>
  );
}
Object.assign(window, {
  renderGapB(surface: string) {
    root.render(<Fixture key={surface} surface={surface} />);
  },
});
