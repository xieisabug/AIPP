use crate::errors::AppError;
use crate::slash::types::SlashInvocation;

pub fn parse_slash_invocations(prompt: &str) -> Result<Vec<SlashInvocation>, AppError> {
    let chars: Vec<(usize, char)> = prompt.char_indices().collect();
    let mut invocations = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        if chars[i].1 != '/' {
            i += 1;
            continue;
        }

        let start = chars[i].0;
        let mut j = i + 1;
        let mut namespace = String::new();

        while j < chars.len() {
            let ch = chars[j].1;
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                namespace.push(ch);
                j += 1;
            } else {
                break;
            }
        }

        if namespace.is_empty() || j >= chars.len() || chars[j].1 != '(' {
            i += 1;
            continue;
        }

        let arg_start = chars[j].0 + '('.len_utf8();
        let mut k = j + 1;
        let mut depth = 1usize;
        let mut escaped = false;

        while k < chars.len() {
            let ch = chars[k].1;

            if escaped {
                escaped = false;
                k += 1;
                continue;
            }

            match ch {
                '\\' => {
                    escaped = true;
                    k += 1;
                }
                '(' => {
                    depth += 1;
                    k += 1;
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = chars[k].0 + ')'.len_utf8();
                        let raw_argument = prompt[arg_start..chars[k].0].to_string();
                        let normalized_argument =
                            normalize_slash_lookup_key(&unescape_slash_argument(&raw_argument));
                        invocations.push(SlashInvocation {
                            namespace: namespace.clone(),
                            raw_argument,
                            normalized_argument,
                            raw_text: prompt[start..end].to_string(),
                            start,
                            end,
                        });
                        i = k + 1;
                        break;
                    }
                    k += 1;
                }
                _ => {
                    k += 1;
                }
            }
        }

        if depth != 0 {
            return Err(AppError::ParseError(format!(
                "Slash 调用语法错误：/{}(...) 缺少闭合右括号",
                namespace
            )));
        }
    }

    Ok(invocations)
}

pub fn remove_slash_invocations_from_prompt(prompt: &str, invocations: &[SlashInvocation]) -> String {
    if invocations.is_empty() {
        return prompt.to_string();
    }

    let mut result = String::with_capacity(prompt.len());
    let mut cursor = 0usize;

    for invocation in invocations {
        if invocation.start > cursor {
            result.push_str(&prompt[cursor..invocation.start]);
        }
        cursor = invocation.end;
    }

    if cursor < prompt.len() {
        result.push_str(&prompt[cursor..]);
    }

    result.trim().to_string()
}

pub fn unescape_slash_argument(argument: &str) -> String {
    let mut result = String::with_capacity(argument.len());
    let mut chars = argument.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek().copied() {
                Some('(' | ')' | '\\') => {
                    result.push(chars.next().unwrap());
                }
                Some(next) => {
                    result.push('\\');
                    result.push(next);
                    chars.next();
                }
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }

    result
}

pub fn escape_slash_argument(argument: &str) -> String {
    let mut escaped = String::with_capacity(argument.len());

    for ch in argument.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '(' => escaped.push_str("\\("),
            ')' => escaped.push_str("\\)"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

pub fn normalize_slash_lookup_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        escape_slash_argument, normalize_slash_lookup_key, parse_slash_invocations,
        remove_slash_invocations_from_prompt, unescape_slash_argument,
    };

    #[test]
    fn test_parse_basic_skill_invocation() {
        let prompt = "请 /skills(React Best Practices) 帮我审查组件";
        let invocations = parse_slash_invocations(prompt).unwrap();

        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].namespace, "skills");
        assert_eq!(invocations[0].raw_argument, "React Best Practices");
        assert_eq!(invocations[0].normalized_argument, "react best practices");
    }

    #[test]
    fn test_parse_nested_parentheses() {
        let prompt = "/skills(React (Vercel) Best Practices)";
        let invocations = parse_slash_invocations(prompt).unwrap();

        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].raw_argument, "React (Vercel) Best Practices");
    }

    #[test]
    fn test_parse_escaped_parentheses_and_backslash() {
        let prompt = r"/skills(Skill Name \(beta\) \\ helper)";
        let invocations = parse_slash_invocations(prompt).unwrap();

        assert_eq!(invocations.len(), 1);
        assert_eq!(
            unescape_slash_argument(&invocations[0].raw_argument),
            r"Skill Name (beta) \ helper"
        );
    }

    #[test]
    fn test_parse_reports_missing_closing_parenthesis() {
        let error = parse_slash_invocations("/skills(React Best Practices").unwrap_err();

        assert_eq!(
            error.to_string(),
            "解析错误: Slash 调用语法错误：/skills(...) 缺少闭合右括号"
        );
    }

    #[test]
    fn test_remove_slash_invocation_from_prompt() {
        let prompt = "请 /skills(React Best Practices) 帮我处理";
        let invocations = parse_slash_invocations(prompt).unwrap();

        assert_eq!(
            remove_slash_invocations_from_prompt(prompt, &invocations),
            "请  帮我处理"
        );
    }

    #[test]
    fn test_remove_leading_skill_invocation_preserves_followup_prompt() {
        let prompt = "/skills(skill-creator) 我想创建一个帮我炒股的skill";
        let invocations = parse_slash_invocations(prompt).unwrap();

        assert_eq!(
            remove_slash_invocations_from_prompt(prompt, &invocations).trim(),
            "我想创建一个帮我炒股的skill"
        );
    }

    #[test]
    fn test_escape_round_trip() {
        let original = r"Skill Name (beta) \ helper";
        let escaped = escape_slash_argument(original);
        let round_trip = unescape_slash_argument(&escaped);

        assert_eq!(round_trip, original);
        assert_eq!(normalize_slash_lookup_key("  React   Best  Practices "), "react best practices");
    }
}
