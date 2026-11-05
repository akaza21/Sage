use std::fs;

#[path = "../build_command_list.rs"]
mod build_command_list;

#[test]
fn browser_webviews_receive_no_tauri_capability() {
    let capabilities = concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities");
    let mut found = false;
    let mut directories = vec![capabilities.into()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            found = true;
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert!(
                value.get("remote").is_none(),
                "{} grants remote origins",
                path.display()
            );
            assert!(
                value.get("windows").is_none(),
                "{} grants a whole window",
                path.display()
            );
            let labels = value["webviews"]
                .as_array()
                .expect("explicit webview scope");
            assert!(!labels.is_empty());
            for label in labels {
                let label = label.as_str().unwrap();
                assert_eq!(
                    label,
                    "main",
                    "{} grants an untrusted webview",
                    path.display()
                );
            }
        }
    }
    assert!(found);
}

#[test]
fn every_browser_command_is_registered_once_behind_the_main_invoke_guard() {
    let source =
        fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ipc_commands.rs")).unwrap();
    let body = source
        .split_once("tauri::generate_handler![")
        .expect("Sage command list")
        .1
        .split_once("];")
        .expect("guarded Sage command list")
        .0;
    let commands: Vec<_> = body
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect();
    let expected = [
        "browser::lifecycle::browser_open",
        "browser::lifecycle::browser_close",
        "browser::clear_data::browser_clear_data",
        "browser::bounds::browser_set_bounds",
        "browser::page_controls::browser_hide",
        "browser::downloads::browser_downloads",
        "browser::downloads::browser_cancel_download",
        "browser::navigation::browser_status",
        "browser::state_query::browser_live_tabs",
        "browser::state_query::browser_history",
        "browser::browsing_history::commands::browser_history_page",
        "browser::browsing_history::commands::browser_history_remove",
        "browser::state_query::browser_suggested_sites",
        "browser::page_controls::browser_navigate",
        "browser::navigation::control::browser_control",
        "browser::page_controls::browser_zoom",
        "browser::device::browser_set_device",
        "browser::page_controls::browser_open_external",
        "browser::page_controls::browser_find",
        "browser::page_controls::browser_print",
        "settings_commands::browser_preferences::browser_preferences",
        "settings_commands::browser_preferences::browser_set_preferences",
        "settings_commands::browser_developer::browser_developer_mode",
        "settings_commands::browser_developer::browser_set_developer_mode",
        "settings_commands::browser_download_folder::browser_download_folder",
        "settings_commands::browser_download_folder::browser_choose_download_folder",
        "settings_commands::browser_download_folder::browser_reset_download_folder",
        "browser::site_permissions::commands::browser_permission_status",
        "browser::site_permissions::commands::browser_permission_resolve",
        "browser::site_permissions::commands::browser_site_settings",
        "browser::site_permissions::commands::browser_site_setting_set",
        "browser::site_permissions::commands::browser_permission_reset",
        "browser::site_permissions::commands::browser_site_settings_reset_all",
        "browser::site_permissions::commands::browser_site_reload_tabs",
        "acp::browser::attachment::attach_browser_tab",
        "acp::browser::attachment::detach_browser_tab",
        "acp::browser::attachment::stop_browser_tab",
        "acp::browser::attachment::browser_attachment_status",
    ];
    for command in expected {
        assert_eq!(
            commands.iter().filter(|entry| **entry == command).count(),
            1,
            "{command}"
        );
    }
    assert!(source.contains("browser::ipc_guard::admits_main_invoke(webview.label(), &url)"));
    assert!(
        source.contains(".reject(\"Sage commands are available only to the main app webview\")")
    );
}
