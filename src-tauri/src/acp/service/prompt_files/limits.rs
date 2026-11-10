//! Limit messages, worded from the caps they name in the units the WebView
//! uses (KB of 1,024 bytes, as `formatSize` in composerPasteText.ts).
use super::{DISPATCH_BYTES, FILE_BYTES, FILE_COUNT, TOTAL_BYTES};
use std::sync::LazyLock;
type Message = LazyLock<String>;
/// The WebView's KB, as `formatSize` divides by.
const KB_BYTES: usize = 1024;
const fn kb(bytes: usize) -> usize {
    bytes / KB_BYTES
}
pub(crate) static PASTE_OVER_TOTAL: Message = LazyLock::new(|| {
    format!(
        "Pasted text exceeds the {} KB attachment limit.",
        kb(TOTAL_BYTES)
    )
});
pub(crate) static OVER_COUNT: Message =
    LazyLock::new(|| format!("Attachments exceed the {FILE_COUNT}-file limit."));
pub(crate) static OVER_TOTAL: Message =
    LazyLock::new(|| format!("Attachments exceed {} KB total.", kb(TOTAL_BYTES)));
pub(crate) static FILE_OVER: Message =
    LazyLock::new(|| format!("A file exceeds the {} KB attachment limit.", kb(FILE_BYTES)));
pub(crate) static READ_OVER: Message = LazyLock::new(|| {
    format!(
        "Attachments exceed {} KB per file or {} KB total.",
        kb(FILE_BYTES),
        kb(TOTAL_BYTES)
    )
});
pub(crate) static DISPATCH_OVER: Message = LazyLock::new(|| {
    format!(
        "Prompt text and file attachments exceed the {} KB native dispatch budget. Shorten the prompt or remove attachments.",
        kb(DISPATCH_BYTES)
    )
});
