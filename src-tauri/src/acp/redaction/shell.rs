#[derive(Debug)]
pub(crate) struct ShellToken {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) cooked: String,
}

/// Boundaries are shell arguments, not whitespace fragments. Spans let us
/// replace a complete argument without reassembling or exposing quoted tails.
pub(crate) fn shell_tokens(input: &str) -> Result<Vec<ShellToken>, ()> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut cooked = String::new();
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if escaped {
            cooked.push(ch);
            escaped = false;
            continue;
        }
        if quote.is_none()
            && (input[index..].starts_with("<<")
                || input[index..].starts_with("@'")
                || input[index..].starts_with("@\""))
        {
            return Err(());
        }
        let escapes = if ch == '\\' {
            quote != Some('\'')
                && (quote != Some('"')
                    || input[index + 1..].starts_with(['"', '\\', '$', '\x60', '\n', '\r']))
        } else {
            ch == '\x60' && quote != Some('\'')
        };
        if escapes {
            start.get_or_insert(index);
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            } else {
                cooked.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            start.get_or_insert(index);
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if let Some(begin) = start.take() {
                tokens.push(ShellToken {
                    start: begin,
                    end: index,
                    cooked: std::mem::take(&mut cooked),
                });
            }
        } else {
            start.get_or_insert(index);
            cooked.push(ch);
        }
    }
    if quote.is_some() || escaped {
        return Err(());
    }
    if let Some(begin) = start {
        tokens.push(ShellToken {
            start: begin,
            end: input.len(),
            cooked,
        });
    }
    Ok(tokens)
}
