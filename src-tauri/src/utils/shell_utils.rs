#[cfg(target_os = "windows")]
use encoding_rs::Encoding;

const POWERSHELL_UTF8_BOOTSTRAP: &str =
    "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); \
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
$OutputEncoding = [System.Text.UTF8Encoding]::new($false);";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Pwsh,
    WindowsPowerShell,
}

#[derive(Debug, Clone)]
pub struct ShellCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl ShellKind {
    pub fn into_command(self, command: &str) -> ShellCommand {
        match self {
            Self::Bash => {
                ShellCommand { program: "bash".to_string(), args: vec!["-c".to_string(), command.to_string()] }
            }
            Self::Zsh => {
                ShellCommand { program: "zsh".to_string(), args: vec!["-c".to_string(), command.to_string()] }
            }
            Self::Pwsh => ShellCommand {
                program: "pwsh".to_string(),
                args: vec!["-Command".to_string(), wrap_powershell_command(command)],
            },
            Self::WindowsPowerShell => ShellCommand {
                program: "powershell".to_string(),
                args: vec!["-Command".to_string(), wrap_powershell_command(command)],
            },
        }
    }
}

pub fn resolve_shell(preferred: Option<&str>) -> Result<ShellKind, String> {
    #[cfg(target_os = "windows")]
    {
        resolve_windows_shell(preferred)
    }
    #[cfg(not(target_os = "windows"))]
    {
        resolve_unix_shell(preferred)
    }
}

pub fn resolve_powershell_shell() -> Result<ShellKind, String> {
    if which::which("pwsh").is_ok() {
        return Ok(ShellKind::Pwsh);
    }
    if which::which("powershell").is_ok() {
        return Ok(ShellKind::WindowsPowerShell);
    }
    Err("Neither pwsh nor powershell was found in PATH".to_string())
}

pub fn wrap_powershell_command(command: &str) -> String {
    format!("{POWERSHELL_UTF8_BOOTSTRAP}\n{command}")
}

pub fn decode_process_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => text,
        Err(_) => {
            #[cfg(target_os = "windows")]
            if let Some(text) = decode_windows_code_page(bytes) {
                return text;
            }

            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

pub fn decode_process_output_line(bytes: &[u8]) -> String {
    decode_process_output(trim_line_endings(bytes))
}

fn trim_line_endings(mut bytes: &[u8]) -> &[u8] {
    while let Some(last) = bytes.last() {
        if *last == b'\n' || *last == b'\r' {
            bytes = &bytes[..bytes.len() - 1];
        } else {
            break;
        }
    }
    bytes
}

#[cfg(target_os = "windows")]
fn resolve_windows_shell(preferred: Option<&str>) -> Result<ShellKind, String> {
    let preferred = preferred.unwrap_or("auto").trim().to_ascii_lowercase();
    match preferred.as_str() {
        "" | "auto" => {
            if which::which("pwsh").is_ok() {
                Ok(ShellKind::Pwsh)
            } else {
                ensure_shell("powershell", ShellKind::WindowsPowerShell)
            }
        }
        "bash" => ensure_shell("bash", ShellKind::Bash),
        "zsh" => ensure_shell("zsh", ShellKind::Zsh),
        "pwsh" => ensure_shell("pwsh", ShellKind::Pwsh),
        "powershell" => ensure_shell("powershell", ShellKind::WindowsPowerShell),
        other => Err(format!("Unsupported DEFAULT_SHELL value: {}", other)),
    }
}

#[cfg(not(target_os = "windows"))]
fn resolve_unix_shell(preferred: Option<&str>) -> Result<ShellKind, String> {
    let preferred = preferred.unwrap_or("auto").trim().to_ascii_lowercase();
    match preferred.as_str() {
        "" | "auto" => {
            if which::which("zsh").is_ok() {
                Ok(ShellKind::Zsh)
            } else {
                ensure_shell("bash", ShellKind::Bash)
            }
        }
        "bash" => ensure_shell("bash", ShellKind::Bash),
        "zsh" => ensure_shell("zsh", ShellKind::Zsh),
        "pwsh" => ensure_shell("pwsh", ShellKind::Pwsh),
        "powershell" => ensure_shell("powershell", ShellKind::WindowsPowerShell),
        other => Err(format!("Unsupported DEFAULT_SHELL value: {}", other)),
    }
}

fn ensure_shell(binary: &str, kind: ShellKind) -> Result<ShellKind, String> {
    which::which(binary)
        .map(|_| kind)
        .map_err(|_| format!("Configured shell '{}' was not found in PATH", binary))
}

#[cfg(target_os = "windows")]
fn decode_windows_code_page(bytes: &[u8]) -> Option<String> {
    let encoding = encoding_for_windows_code_page(current_windows_code_page())?;
    let (decoded, _, _) = encoding.decode(bytes);
    Some(decoded.into_owned())
}

#[cfg(target_os = "windows")]
fn encoding_for_windows_code_page(code_page: u32) -> Option<&'static Encoding> {
    match code_page {
        65001 => Encoding::for_label(b"utf-8"),
        936 => Encoding::for_label(b"gbk"),
        54936 => Encoding::for_label(b"gb18030"),
        932 => Encoding::for_label(b"shift_jis"),
        949 => Encoding::for_label(b"euc-kr"),
        950 => Encoding::for_label(b"big5"),
        1250 => Encoding::for_label(b"windows-1250"),
        1251 => Encoding::for_label(b"windows-1251"),
        1252 => Encoding::for_label(b"windows-1252"),
        1253 => Encoding::for_label(b"windows-1253"),
        1254 => Encoding::for_label(b"windows-1254"),
        1255 => Encoding::for_label(b"windows-1255"),
        1256 => Encoding::for_label(b"windows-1256"),
        1257 => Encoding::for_label(b"windows-1257"),
        1258 => Encoding::for_label(b"windows-1258"),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn current_windows_code_page() -> u32 {
    unsafe { GetACP() }
}

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn GetACP() -> u32;
}

#[cfg(test)]
mod tests {
    use super::{decode_process_output, resolve_powershell_shell, resolve_shell, ShellKind};

    #[test]
    fn test_decode_utf8_output() {
        assert_eq!(decode_process_output("中文".as_bytes()), "中文");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_decode_gbk_output() {
        assert_eq!(decode_process_output(&[0xD6, 0xD0, 0xCE, 0xC4]), "中文");
    }

    #[test]
    fn test_resolve_powershell_shell() {
        let shell = resolve_powershell_shell();
        if which::which("pwsh").is_ok() || which::which("powershell").is_ok() {
            let shell = shell.expect("PowerShell should be available for tests");
            assert!(matches!(shell, ShellKind::Pwsh | ShellKind::WindowsPowerShell));
        } else {
            assert!(shell.is_err());
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_resolve_auto_shell_on_windows() {
        let shell = resolve_shell(Some("auto")).expect("auto shell should resolve");
        if which::which("pwsh").is_ok() {
            assert_eq!(shell, ShellKind::Pwsh);
        } else {
            assert_eq!(shell, ShellKind::WindowsPowerShell);
        }
    }
}
