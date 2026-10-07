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
