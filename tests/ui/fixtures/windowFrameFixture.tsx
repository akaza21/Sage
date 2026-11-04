import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { TitleBar } from "../../../src/workbench/TitleBar";
import "../../../src/styles/global.css";

export function WindowFrameFixture() {
  const [mounted, setMounted] = useState(true);
  const [generation, setGeneration] = useState(0);
  return (
    <>
      {mounted && (
        <TitleBar key={generation} mode="agent" onModeChange={() => {}} />
      )}
      <button onClick={() => setMounted(!mounted)}>Toggle title bar</button>
      <button onClick={() => setGeneration(generation + 1)}>
        Replace title bar
      </button>
      <section aria-label="Transcript" />
      <section aria-label="Editor" />
    </>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <WindowFrameFixture />
  </StrictMode>,
);
