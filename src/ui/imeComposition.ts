/** A React or native keydown, as far as composition goes. */
export interface ComposingKeyEvent {
  keyCode: number;
  isComposing?: boolean;
  nativeEvent?: { isComposing: boolean };
}

/**
 * True while an input method editor owns the key, so Enter and Tab confirm
 * its text instead of sending or picking. Safari and WebView2 report the
 * keydown that ends a composition only as keyCode 229, with isComposing
 * already false.
 */
export function isImeComposing(event: ComposingKeyEvent): boolean {
  return (
    (event.nativeEvent ?? event).isComposing === true || event.keyCode === 229
  );
}
