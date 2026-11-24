/// Parse an editable stdio command into literal arguments. No shell is involved.
pub(crate) fn split_command_line(input: &str) -> Vec<String> {
    parse(input, cfg!(windows))
}

fn parse(input: &str, windows: bool) -> Vec<String> {
    let mut args = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if quote.is_none() && c.is_ascii_whitespace() {
            if started {
                args.push(std::mem::take(&mut word));
                started = false;
            }
            continue;
        }
        started = true;
        if quote == Some(c) {
            quote = None;
        } else if quote.is_none() && matches!(c, '"' | '\'') {
            quote = Some(c);
        } else if c == '\\' && quote == Some('\'') && chars.next_if_eq(&'\'').is_some() {
            word.push('\'');
        } else if c != '\\' || quote == Some('\'') {
            word.push(c);
        } else if windows {
            let mut count = 1;
            while chars.next_if_eq(&'\\').is_some() {
                count += 1;
            }
            // Windows paths (including UNC) keep backslashes. Immediately before
            // a double quote, pairs represent literal slashes and an odd one escapes it.
            if chars.peek() == Some(&'"') {
                word.extend(std::iter::repeat_n('\\', count / 2));
                if count % 2 == 1 {
                    word.push(chars.next().unwrap());
                }
            } else if count == 1 && quote.is_none() && chars.peek().is_some_and(|next|
                *next == '\'' || (next.is_ascii_whitespace() && !word.contains(['\\', ':']))
            ) {
                word.push(chars.next().unwrap());
            } else {
                word.extend(std::iter::repeat_n('\\', count));
            }
        } else {
            word.push(chars.next().unwrap_or('\\'));
        }
    }
    if started { args.push(word); }
    args
}

#[cfg(test)]
#[path = "stdio_args_tests.rs"]
mod tests;
