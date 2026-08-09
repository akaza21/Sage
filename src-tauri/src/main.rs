#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    sage_lib::run_workspace_search_helper_if_requested();
    sage_lib::run();
}
