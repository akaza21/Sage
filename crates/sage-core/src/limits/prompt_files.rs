//! Bounds on the files a prompt carries: what one staging accepts and what one
//! dispatch sends, prompt text and attached context included.

pub const FILE_BYTES: usize = 64 * 1024;
pub const TOTAL_BYTES: usize = 96 * 1024;
pub const FILE_COUNT: usize = 16;
/// Prompt text, attached context and staged files together, per dispatch.
pub const DISPATCH_BYTES: usize = 120 * 1024;
