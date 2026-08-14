#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(sage_lib::codex_helper_boundary::run_helper_process(
        std::env::args_os().skip(1),
    ));
}

#[cfg(not(windows))]
fn main() {
    std::process::exit(2);
}
