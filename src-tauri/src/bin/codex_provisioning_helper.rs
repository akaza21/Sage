#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
// This feature-only binary shares the bilateral protocol source with the caller,
// but its process entrypoint can reach only the server half.
#[allow(dead_code, unused_imports)]
#[path = "../codex_helper_boundary.rs"]
mod codex_helper_boundary;

#[cfg(windows)]
fn main() {
    std::process::exit(codex_helper_boundary::run_helper_process(
        std::env::args_os().skip(1),
    ));
}

#[cfg(not(windows))]
fn main() {
    std::process::exit(2);
}
