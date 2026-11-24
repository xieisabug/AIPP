use super::background_process::{background_command, background_group_command};
use std::ffi::c_void;

#[link(name = "kernel32")]
extern "system" {
    fn GetConsoleWindow() -> *mut c_void;
}
#[link(name = "user32")]
extern "system" {
    fn IsWindowVisible(window: *mut c_void) -> i32;
}

#[test]
fn test_background_process_windows_console_and_pipes() {
    const CHILD: &str = "AIPP_TEST_BACKGROUND_CONSOLE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        // The child observes its own console, including one allocated by Windows on spawn.
        let window = unsafe { GetConsoleWindow() };
        assert!(window.is_null() || unsafe { IsWindowVisible(window) } == 0);
        println!("child stdout ready");
        eprintln!("child stderr ready");
        return;
    }

    let exe = std::env::current_exe().unwrap();
    for mut command in [background_command(&exe), background_group_command(&exe)] {
        let output = command
            .env(CHILD, "1")
            .args(["test_background_process_windows_console_and_pipes", "--nocapture"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("child stdout ready"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("child stderr ready"));
    }
}
