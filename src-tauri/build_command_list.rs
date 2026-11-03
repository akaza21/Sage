pub fn commands_from_checkout(manifest_dir: &std::path::Path) -> Result<Vec<String>, String> {
    // A shared Cargo target can reuse this build-script binary in another checkout.
    let source = std::fs::read_to_string(manifest_dir.join("src/ipc_commands.rs"))
        .map_err(|error| format!("read active Sage command registry: {error}"))?;
    let body = source
        .split_once("tauri::generate_handler![")
        .ok_or("Sage command handler must exist")?
        .1
        .split_once("];")
        .ok_or("Sage command handler must close inside the main-webview guard")?
        .0;
    parse_handler_body(body).map(|commands| commands.into_iter().map(str::to_owned).collect())
}

pub fn parse_handler_body(body: &str) -> Result<Vec<&str>, String> {
    let mut commands = Vec::new();
    for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line.starts_with("#[") {
            let remainder = line
                .split_once(']')
                .ok_or_else(|| format!("unterminated command attribute: {line}"))?
                .1;
            if !remainder.trim().is_empty() {
                return Err(format!(
                    "command attribute must occupy its own line: {line}"
                ));
            }
            continue;
        }
        let path = line.trim_end_matches(',');
        if path.is_empty()
            || !path.split("::").all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                    && !segment.as_bytes()[0].is_ascii_digit()
            })
        {
            return Err(format!("invalid Sage command path: {line}"));
        }
        let command = path.rsplit("::").next().expect("nonempty path");
        if commands.contains(&command) {
            return Err(format!("duplicate Sage command: {command}"));
        }
        commands.push(command);
    }
    if commands.is_empty() {
        return Err("Sage command list cannot be empty".into());
    }
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reused_build_script_reads_commands_from_the_active_checkout() {
        let root =
            std::env::temp_dir().join(format!("sage-command-manifest-{}", std::process::id()));
        let first = root.join("main");
        let second = root.join("agent");
        for checkout in [&first, &second] {
            std::fs::create_dir_all(checkout.join("src")).unwrap();
        }
        std::fs::write(
            first.join("src/ipc_commands.rs"),
            "tauri::generate_handler![\nchat_commands::rename_chat,\n];",
        )
        .unwrap();
        std::fs::write(
            second.join("src/ipc_commands.rs"),
            "tauri::generate_handler![\nchat_commands::rename_chat,\ncommands::acp::turn_workspace::get_acp_turn_workspace_changes,\n];",
        )
        .unwrap();
        assert_eq!(commands_from_checkout(&first).unwrap(), ["rename_chat"]);
        assert_eq!(
            commands_from_checkout(&second).unwrap(),
            ["rename_chat", "get_acp_turn_workspace_changes"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_line_cfg_command_is_rejected_not_silently_omitted() {
        assert!(parse_handler_body("#[cfg(windows)] browser::browser_open,").is_err());
    }

    #[test]
    fn separate_cfg_line_retains_the_command() {
        assert_eq!(
            parse_handler_body("#[cfg(windows)]\nbrowser::browser_open,").unwrap(),
            vec!["browser_open"]
        );
    }
}
