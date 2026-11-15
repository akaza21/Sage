import { create } from "zustand";

export interface InheritedComposerModel {
  model: string;
  effort: string | null;
  id?: string;
}

export const useInheritedStore = create<
  Readonly<Record<string, InheritedComposerModel>>
>(() => ({}));

export function useInheritedComposerModel(chatId: string | null) {
  return useInheritedStore((state) => (chatId ? state[chatId] : undefined));
}
