mod build_command_list;

fn main() {
    let commands = sage_commands();
    write_main_command_permission(commands);
    let app_manifest = tauri_build::AppManifest::new().commands(commands);
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rerun-if-changed=windows-app.manifest");
        let manifest =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        tauri_build::try_build(
            tauri_build::Attributes::new()
                .app_manifest(app_manifest)
                .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
        )
        .expect("failed to build Tauri resources");
    }
    #[cfg(not(windows))]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(app_manifest))
        .expect("failed to build Tauri resources");
}

fn sage_commands() -> &'static [&'static str] {
    println!("cargo:rerun-if-changed=src/ipc_commands.rs");
    let source = include_str!("src/ipc_commands.rs");
    let body = source
        .split_once("tauri::generate_handler![")
        .expect("Sage command handler must exist")
        .1
        .split_once("];")
        .expect("Sage command handler must close inside the main-webview guard")
        .0;
    let commands = build_command_list::parse_handler_body(body)
        .expect("Sage command manifest must exactly match the handler");
    Box::leak(commands.into_boxed_slice())
}

fn write_main_command_permission(commands: &[&str]) {
    let mut permission = String::from(
        "# Generated from Sage's invoke handler by build.rs.\n[[set]]\nidentifier = \"main-commands\"\ndescription = \"Sage app commands for the trusted main webview.\"\npermissions = [\n",
    );
    for command in commands {
        permission.push_str(&format!("  \"allow-{}\",\n", command.replace('_', "-")));
    }
    permission.push_str("]\n");
    let path = std::path::Path::new("permissions/main-commands.toml");
    std::fs::create_dir_all(path.parent().expect("permission parent"))
        .expect("create permissions directory");
    if std::fs::read_to_string(path).ok().as_deref() != Some(permission.as_str()) {
        std::fs::write(path, permission).expect("write main command permission");
    }
}
