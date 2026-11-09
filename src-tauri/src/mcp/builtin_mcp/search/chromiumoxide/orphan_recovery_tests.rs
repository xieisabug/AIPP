use super::*;

fn orphan() -> BrowserProcess {
    BrowserProcess { pid: 50588, parent_alive: false, name: "chrome.exe".into(),
        command_line: r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --remote-debugging-port=0 --user-data-dir="C:\Users\admin\AIPP profile" --headless --enable-automation"#.into(), created: "123".into() }
}

#[test]
fn test_search_recovery_identifies_only_orphan_automation_for_exact_profile() {
    let process = orphan();
    assert!(is_orphan_search_browser(&process, r"C:\Users\admin\AIPP profile"));
    assert!(!is_orphan_search_browser(&process, r"C:\Users\admin\AIPP"));
    let mut active = process.clone();
    active.parent_alive = true;
    assert!(!is_orphan_search_browser(&active, r"C:\Users\admin\AIPP profile"));
    let mut personal = process.clone();
    personal.command_line = personal.command_line.replace("--enable-automation", "");
    assert!(!is_orphan_search_browser(&personal, r"C:\Users\admin\AIPP profile"));
    let mut child = process;
    child.command_line.push_str(" --type=renderer");
    assert!(!is_orphan_search_browser(&child, r"C:\Users\admin\AIPP profile"));
}
