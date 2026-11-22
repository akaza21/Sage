import { lucideIcon as leanIcon } from "./leanIconCore";

// Icons only lazily loaded parts draw. They are imported from here, never
// through leanIcons, so the startup chunks that hold that barrel skip them.
export const ArrowDownUp = leanIcon("arrow-down-up", [
  "m3 16 4 4 4-4",
  "M7 20V4",
  "m21 8-4-4-4 4",
  "M17 4v16",
]);
export const ChevronLeft = leanIcon("chevron-left", ["m15 18-6-6 6-6"]);
export const CirclePlus = leanIcon("circle-plus", [
  ["circle", { cx: "12", cy: "12", r: "10" }],
  "M8 12h8",
  "M12 8v8",
]);
export const Download = leanIcon("download", [
  "M12 15V3",
  "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4",
  "m7 10 5 5 5-5",
]);
export const FolderInput = leanIcon("folder-input", [
  "M2 9V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-1",
  "M2 13h10",
  "m9 16 3-3-3-3",
]);
export const Folders = leanIcon("folders", [
  "M20 5a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h2.5a1.5 1.5 0 0 1 1.2.6l.6.8a1.5 1.5 0 0 0 1.2.6z",
  "M3 8.268a2 2 0 0 0-1 1.738V19a2 2 0 0 0 2 2h11a2 2 0 0 0 1.732-1",
]);
export const Lightbulb = leanIcon("lightbulb", [
  "M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5",
  "M9 18h6",
  "M10 22h4",
]);
export const List = leanIcon("list", [
  "M3 5h.01",
  "M3 12h.01",
  "M3 19h.01",
  "M8 5h13",
  "M8 12h13",
  "M8 19h13",
]);
export const ListChecks = leanIcon("list-checks", [
  "M13 5h8",
  "M13 12h8",
  "M13 19h8",
  "m3 17 2 2 4-4",
  "m3 7 2 2 4-4",
]);
export const ListFilter = leanIcon("list-filter", [
  "M2 5h20",
  "M6 12h12",
  "M9 19h6",
]);
export const MessageCircleQuestion = leanIcon("message-circle-question-mark", [
  "M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719",
  "M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3",
  "M12 17h.01",
]);
export const PanelLeft = leanIcon("panel-left", [
  ["rect", { width: "18", height: "18", x: "3", y: "3", rx: "2" }],
  "M9 3v18",
]);
export const Paperclip = leanIcon("paperclip", [
  "m16 6-8.414 8.586a2 2 0 0 0 2.829 2.829l8.414-8.586a4 4 0 1 0-5.657-5.657l-8.379 8.551a6 6 0 1 0 8.485 8.485l8.379-8.551",
]);
export const Pause = leanIcon("pause", [
  ["rect", { x: "14", y: "3", width: "5", height: "18", rx: "1" }],
  ["rect", { x: "5", y: "3", width: "5", height: "18", rx: "1" }],
]);
export const RotateCw = leanIcon("rotate-cw", [
  "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8",
  "M21 3v5h-5",
]);
export const SquareArrowOutUpRight = leanIcon("square-arrow-out-up-right", [
  "M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6",
  "m21 3-9 9",
  "M15 3h6v6",
]);
export const Target = leanIcon("target", [
  ["circle", { cx: "12", cy: "12", r: "10" }],
  ["circle", { cx: "12", cy: "12", r: "6" }],
  ["circle", { cx: "12", cy: "12", r: "2" }],
]);
export const Unlink2 = leanIcon("unlink2 lucide-unlink-2", [
  "M15 7h2a5 5 0 0 1 0 10h-2m-6 0H7A5 5 0 0 1 7 7h2",
]);
