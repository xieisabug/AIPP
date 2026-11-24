use super::parse;

#[test]
fn test_stdio_args_windows_native_and_unc_paths() {
    let cmd = r#""C:\Program Files\nodejs\node.exe" "\\host\share name\tool.js" C:\dir\ --flag"#;
    assert_eq!(parse(cmd, true), [r"C:\Program Files\nodejs\node.exe", r"\\host\share name\tool.js", r"C:\dir\", "--flag"]);
}

#[test]
fn test_stdio_args_preserve_quoted_argv_from_json_import() {
    let cmd = r#"'C:\Program Files\node.exe' 'C:\team\'s files'"\\" '' 'a"b' --foo"#;
    for windows in [false, true] {
        assert_eq!(parse(cmd, windows), [r"C:\Program Files\node.exe", r"C:\team's files\", "", "a\"b", "--foo"]);
    }
}

#[test]
fn test_stdio_args_keep_escaped_spaces_and_quotes() {
    for windows in [false, true] {
        assert_eq!(parse(r#"tool hello\ world "say \"hi\"""#, windows), ["tool", "hello world", "say \"hi\""]);
        assert_eq!(parse(r"tool 'it\'s fine' --next", windows), ["tool", "it's fine", "--next"]);
    }
    assert_eq!(parse(r#"tool "C:\dir with space\\" --x"#, true), ["tool", r"C:\dir with space\", "--x"]);
}
