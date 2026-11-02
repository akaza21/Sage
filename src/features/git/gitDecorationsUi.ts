/**
 * Everything Git decorations (EP-13) render with, loaded as one lazy chunk
 * only while `git.decorations` is on and a repository root is open.
 */
export { gitDecorationAt, rootGitDecorations } from "./gitDecorations";
export { GitDecorationBadge } from "./GitDecorationBadge";
export { GitTabTitle } from "./GitTabTitle";
