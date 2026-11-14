export interface ComposingKeyEvent {
  keyCode: number;
  isComposing?: boolean;
  nativeEvent?: { isComposing: boolean };
}

/**
 * Safari and WebView2 report the keydown that ends a composition only as
 * keyCode 229, with isComposing already false.
 */
export function isImeComposing(event: ComposingKeyEvent): boolean {
  return (
    (event.nativeEvent ?? event).isComposing === true || event.keyCode === 229
  );
}
