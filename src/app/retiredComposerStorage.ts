export {};

(() => {
  try {
    window.localStorage.removeItem("sage:composer:model-favorites:v1");
  } catch {
    return;
  }
})();
