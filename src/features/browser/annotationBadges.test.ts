import { afterEach, expect, it } from "vitest";
import source from "./annotationBadges.js?raw";

const install = new Function(
  `return (${source.replace("export default ", "")})`,
)();
const controls = () => Reflect.get(globalThis, "__sageAnnotationBadges");
afterEach(() => {
  controls()?.stop();
  Reflect.deleteProperty(globalThis, "__sageAnnotationBadges");
});

it("suspends badge DOM for capture and restores it independently of Annotate input", () => {
  install([{ ordinal: 1, box: { x: 10, y: 20 } }], location.href);
  const host = document.documentElement.lastElementChild!;
  expect(host.shadowRoot).toBeNull();
  controls().pause();
  expect(host.isConnected).toBe(false);
  controls().resume();
  expect(host.isConnected).toBe(true);
  controls().stop();
  expect(host.isConnected).toBe(false);
});

it("replaces the previous overlay and clears the page when the draft empties", () => {
  install([{ ordinal: 1, box: { x: 10, y: 20 } }], location.href);
  const previous = document.documentElement.lastElementChild!;
  install([], location.href);
  expect(previous.isConnected).toBe(false);
  controls().resume();
  expect(previous.isConnected).toBe(false);
});
