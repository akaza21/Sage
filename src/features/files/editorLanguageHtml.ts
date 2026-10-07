import { htmlLanguage } from "@codemirror/lang-html";

export const loadHtmlEditorLanguage = () =>
  htmlLanguage.configure({ dialect: "noMatch" });
